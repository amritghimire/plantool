use anyhow::{bail, Context};
use base64::Engine;
use plantool_core::Provider;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

const LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Debug)]
pub struct Attachment {
    pub path: PathBuf,
    pub extension: String,
}

pub fn parse(text: &str, roots: &[PathBuf]) -> anyhow::Result<Vec<Attachment>> {
    let mut files = Vec::new();
    for path in text
        .lines()
        .filter_map(|line| line.strip_prefix("Attached file: "))
    {
        let path = Path::new(path.trim());
        if !path.is_absolute() {
            bail!("Attachment paths must be absolute session upload paths");
        }
        let path = path
            .canonicalize()
            .with_context(|| format!("Attachment is missing: {}", path.display()))?;
        let allowed = roots.iter().any(|root| {
            let Ok(root) = root.canonicalize() else {
                return false;
            };
            let Ok(dir) = root.join("attachments").canonicalize() else {
                return false;
            };
            dir.starts_with(&root) && path.starts_with(&dir)
        });
        if !allowed {
            bail!(
                "Attachment is outside this session's attachment directory: {}",
                path.display()
            );
        }
        let metadata = path.metadata()?;
        if !metadata.is_file() {
            bail!("Attachment must be a regular file: {}", path.display());
        }
        if metadata.len() == 0 || metadata.len() > LIMIT {
            bail!(
                "Each attachment must be between 1 byte and 10 MB: {}",
                path.display()
            );
        }
        if !files.iter().any(|file: &Attachment| file.path == path) {
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            files.push(Attachment { path, extension });
        }
    }
    Ok(files)
}

fn image_mime(extension: &str) -> Option<&'static str> {
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn is_image(extension: &str) -> bool {
    image_mime(extension).is_some()
        || matches!(
            extension,
            "svg" | "bmp" | "tif" | "tiff" | "heic" | "heif" | "avif" | "ico"
        )
}

fn image_bytes(
    file: &Attachment,
    provider: Provider,
) -> anyhow::Result<Option<(Vec<u8>, &'static str)>> {
    let Some(mime) = image_mime(&file.extension) else {
        if is_image(&file.extension) {
            bail!("{} cannot receive .{} images natively. Convert the image to PNG, JPEG, GIF, or WebP.", provider.as_str(), file.extension);
        }
        return Ok(None);
    };
    let mut bytes = Vec::new();
    std::fs::File::open(&file.path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        bail!("Each attachment must be at most 10 MB");
    }
    let valid = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    if !valid {
        bail!(
            "Attachment is not a valid {mime} image: {}",
            file.path.display()
        );
    }
    Ok(Some((bytes, mime)))
}

pub fn validate(text: &str, roots: &[PathBuf], provider: Provider) -> anyhow::Result<()> {
    let files = parse(text, roots)?;
    if matches!(provider, Provider::Codex | Provider::Claude) {
        for file in files {
            image_bytes(&file, provider)?;
        }
    }
    Ok(())
}

pub fn codex_input(text: &str, roots: &[PathBuf]) -> anyhow::Result<Vec<Value>> {
    let mut content = vec![json!({ "type": "text", "text": text, "text_elements": [] })];
    for file in parse(text, roots)? {
        if image_bytes(&file, Provider::Codex)?.is_some() {
            content.push(json!({ "type": "localImage", "path": file.path }));
        }
    }
    Ok(content)
}

pub fn claude_message(text: &str, roots: &[PathBuf]) -> anyhow::Result<Value> {
    let mut content = vec![json!({ "type": "text", "text": text })];
    for file in parse(text, roots)? {
        if let Some((bytes, mime)) = image_bytes(&file, Provider::Claude)? {
            content.push(json!({ "type": "image", "source": { "type": "base64", "media_type": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes) } }));
        }
    }
    Ok(json!({ "type": "user", "message": { "role": "user", "content": content } }))
}

pub fn file_args(
    text: &str,
    roots: &[PathBuf],
    provider: Provider,
) -> anyhow::Result<Vec<PathBuf>> {
    Ok(parse(text, roots)?
        .into_iter()
        .filter(|file| {
            provider != Provider::Copilot
                || matches!(
                    file.extension.as_str(),
                    "jpg" | "jpeg" | "png" | "gif" | "webp" | "pdf" | "heic" | "heif"
                )
        })
        .map(|file| file.path)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf, String) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("attachments");
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("image.PNG");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\nfixture").unwrap();
        let text = format!("Inspect this\nAttached file: {}", path.display());
        (root, path, text)
    }
    #[test]
    fn native_payloads_keep_text_and_encode_image() {
        let (root, path, text) = fixture();
        let roots = vec![root.path().to_path_buf()];
        let input = codex_input(&text, &roots).unwrap();
        assert_eq!(input[0]["text"], text);
        assert_eq!(
            input[1],
            json!({"type":"localImage", "path":path.canonicalize().unwrap()})
        );
        let message = claude_message(&text, &roots).unwrap();
        assert_eq!(message["message"]["content"][0]["text"], text);
        let source = &message["message"]["content"][1]["source"];
        assert_eq!(source["media_type"], "image/png");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(source["data"].as_str().unwrap())
                .unwrap(),
            std::fs::read(path).unwrap()
        );
    }
    #[test]
    fn validates_paths_limits_and_duplicates() {
        let (root, path, text) = fixture();
        let roots = vec![root.path().to_path_buf()];
        assert_eq!(parse(&format!("{text}\n{text}"), &roots).unwrap().len(), 1);
        assert!(parse("Attached file: relative.png", &roots).is_err());
        assert!(parse("Attached file: /missing/file", &roots).is_err());
        let outside = root.path().join("outside.png");
        std::fs::write(&outside, b"image").unwrap();
        assert!(parse(&format!("Attached file: {}", outside.display()), &roots).is_err());
        std::fs::write(&path, []).unwrap();
        assert!(parse(&text, &roots).is_err());
        std::fs::File::create(&path)
            .unwrap()
            .set_len(LIMIT + 1)
            .unwrap();
        assert!(parse(&text, &roots).is_err());
        assert!(parse(
            &format!(
                "Attached file: {}",
                root.path().join("attachments").display()
            ),
            &roots
        )
        .is_err());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_and_cross_session_paths() {
        let (root, _, text) = fixture();
        let other = tempfile::tempdir().unwrap();
        assert!(parse(&text, &[other.path().to_path_buf()]).is_err());
        let outside = root.path().join("outside.png");
        std::fs::write(&outside, b"image").unwrap();
        let link = root.path().join("attachments/link.png");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(parse(
            &format!("Attached file: {}", link.display()),
            &[root.path().to_path_buf()]
        )
        .is_err());
        let alternate = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(
            root.path().join("attachments"),
            alternate.path().join("attachments"),
        )
        .unwrap();
        assert!(parse(&text, &[alternate.path().to_path_buf()]).is_err());
    }
    #[test]
    fn rejects_bad_and_unsupported_images_but_keeps_ordinary_files() {
        let (root, path, text) = fixture();
        let roots = vec![root.path().to_path_buf()];
        std::fs::write(path, b"not a png").unwrap();
        assert!(validate(&text, &roots, Provider::Codex).is_err());
        assert!(validate(&text, &roots, Provider::Claude).is_err());
        let svg = root.path().join("attachments/image.svg");
        std::fs::write(&svg, "<svg/>").unwrap();
        let svg_text = format!("Attached file: {}", svg.display());
        assert!(codex_input(&svg_text, &roots)
            .unwrap_err()
            .to_string()
            .contains("Convert"));
        assert!(claude_message(&svg_text, &roots).is_err());
        let doc = root.path().join("attachments/notes.txt");
        std::fs::write(&doc, "notes").unwrap();
        let text = format!("Attached file: {}", doc.display());
        assert_eq!(codex_input(&text, &roots).unwrap().len(), 1);
        assert_eq!(
            claude_message(&text, &roots).unwrap()["message"]["content"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn preserves_provider_file_selection_for_followups() {
        let (root, path, image) = fixture();
        let doc = root.path().join("attachments/notes.txt");
        let pdf = root.path().join("attachments/report.pdf");
        std::fs::write(&doc, "notes").unwrap();
        std::fs::write(&pdf, "%PDF").unwrap();
        let roots = vec![root.path().to_path_buf()];
        for text in [
            image.clone(),
            format!(
                "Followup\n{image}\nAttached file: {}\nAttached file: {}",
                doc.display(),
                pdf.display()
            ),
        ] {
            let copilot = file_args(&text, &roots, Provider::Copilot).unwrap();
            assert!(copilot.contains(&path.canonicalize().unwrap()));
            assert!(!copilot.contains(&doc));
            let opencode = file_args(&text, &roots, Provider::Ollama).unwrap();
            assert_eq!(opencode.len(), parse(&text, &roots).unwrap().len());
        }
    }
}
