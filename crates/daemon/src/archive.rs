use crate::registry::LiveSession;
use plantool_core::{DocKind, DocRevision, Run, Session, State};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

#[derive(Serialize, Deserialize)]
pub struct Bundle {
    pub version: u32,
    pub session: Session,
    pub state: State,
    pub documents: Vec<DocRevision>,
    pub revisions: Vec<DocRevision>,
    pub runs: Vec<Run>,
    pub transcripts: BTreeMap<String, String>,
}

pub fn summary(bundle: &Bundle, note: Option<&str>) -> String {
    let mut out = format!(
        "# {}\n\nSession: {}\nStep: {}\nOpen comments: {}\n",
        bundle.session.title,
        bundle.session.key(),
        bundle.state.stage,
        bundle
            .state
            .comments
            .iter()
            .filter(|c| c.parent.is_none() && !c.resolved)
            .count()
    );
    for document in &bundle.documents {
        let mut include = false;
        for line in document.content.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                include = [
                    "Summary",
                    "Decisions",
                    "Risks",
                    "Assumptions",
                    "Decisions needed",
                ]
                .contains(&heading.trim());
            }
            if include {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        out.push_str("\n## Owner note\n\n");
        out.push_str(note);
        out.push('\n');
    }
    out
}

pub fn export(
    session: &LiveSession,
    transcripts: bool,
    revisions: bool,
    note: Option<&str>,
) -> anyhow::Result<Vec<u8>> {
    let state = session.state();
    let documents: Vec<_> = DocKind::ALL
        .into_iter()
        .filter_map(|k| session.doc(k))
        .collect();
    let mut saved = BTreeMap::new();
    for doc in &documents {
        saved.insert((doc.kind, doc.sha.clone()), doc.clone());
    }
    if let Some(approved) = &state.approved {
        if let Some(doc) = session.doc_revision(DocKind::Plan, &approved.sha)? {
            saved.insert((doc.kind, doc.sha.clone()), doc);
        }
    }
    for (kind, sha) in &state.viewed {
        if let Some(doc) = session.doc_revision(*kind, sha)? {
            saved.insert((doc.kind, doc.sha.clone()), doc);
        }
    }
    if revisions {
        for kind in DocKind::ALL {
            let dir = session.store.dir.join("revisions").join(kind.as_str());
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries {
                    let path = entry?.path();
                    if path.extension().and_then(|p| p.to_str()) != Some("md") {
                        continue;
                    }
                    if let Some(sha) = path.file_stem().and_then(|p| p.to_str()) {
                        if let Some(doc) = session.doc_revision(kind, sha)? {
                            saved.insert((kind, sha.into()), doc);
                        }
                    }
                }
            }
        }
    }
    let runs = session.runs();
    let mut logs = BTreeMap::new();
    if transcripts {
        for run in &runs {
            let path = session.store.run_log_path(&run.id);
            if path.is_file() {
                logs.insert(run.id.clone(), std::fs::read_to_string(path)?);
            }
        }
    }
    let bundle = Bundle {
        version: 1,
        session: session.session(),
        state,
        documents,
        revisions: saved.into_values().collect(),
        runs,
        transcripts: logs,
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("session.json", options)?;
    zip.write_all(&serde_json::to_vec_pretty(&bundle)?)?;
    zip.start_file("summary.md", options)?;
    zip.write_all(summary(&bundle, note).as_bytes())?;
    for doc in &bundle.documents {
        zip.start_file(doc.kind.file_name(), options)?;
        zip.write_all(doc.content.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<Bundle> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    if zip.len() > 20 {
        anyhow::bail!("too many archive entries");
    }
    let mut names = std::collections::HashSet::new();
    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        if file.enclosed_name().is_none()
            || file.name().contains('\\')
            || file.is_dir()
            || file.size() > 64 * 1024 * 1024
        {
            anyhow::bail!("invalid archive entry");
        }
        let allowed = file.name() == "session.json"
            || file.name() == "summary.md"
            || DocKind::from_file_name(file.name()).is_some();
        if !names.insert(file.name().to_string()) {
            anyhow::bail!("duplicate archive entry");
        }
        if !allowed {
            anyhow::bail!("unexpected archive entry");
        }
    }
    let mut content = String::new();
    zip.by_name("session.json")?
        .take(64 * 1024 * 1024 + 1)
        .read_to_string(&mut content)?;
    if content.len() > 64 * 1024 * 1024 {
        anyhow::bail!("archive is too large");
    }
    let bundle: Bundle = serde_json::from_str(&content)?;
    if bundle.version != 1 {
        anyhow::bail!("unsupported archive version");
    }
    if !crate::registry::valid_slug(&bundle.session.slug) {
        anyhow::bail!("invalid session slug");
    }
    for doc in bundle.revisions.iter().chain(&bundle.documents) {
        if doc.sha != plantool_core::sha256_hex(&doc.content) {
            anyhow::bail!("revision checksum mismatch");
        }
    }
    let valid_sha = |sha: &str| sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit());
    for sha in bundle
        .state
        .docs
        .values()
        .map(|d| d.sha.as_str())
        .chain(bundle.state.viewed.values().map(String::as_str))
        .chain(bundle.state.approved.iter().map(|a| a.sha.as_str()))
    {
        if !valid_sha(sha) {
            anyhow::bail!("invalid stored revision sha");
        }
    }
    for (kind, sha) in bundle
        .state
        .viewed
        .iter()
        .map(|(kind, sha)| (*kind, sha))
        .chain(
            bundle
                .state
                .approved
                .iter()
                .map(|a| (DocKind::Plan, &a.sha)),
        )
        .chain(
            bundle
                .state
                .docs
                .iter()
                .map(|(kind, revision)| (*kind, &revision.sha)),
        )
    {
        if !bundle
            .revisions
            .iter()
            .chain(&bundle.documents)
            .any(|doc| doc.kind == kind && &doc.sha == sha)
        {
            anyhow::bail!("archive is missing a referenced revision");
        }
    }
    for run in &bundle.runs {
        if !crate::registry::valid_slug(&run.id) {
            anyhow::bail!("invalid run id");
        }
    }
    for id in bundle.transcripts.keys() {
        if !crate::registry::valid_slug(id) {
            anyhow::bail!("invalid transcript id");
        }
    }
    Ok(bundle)
}
