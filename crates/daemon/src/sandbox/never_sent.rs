use anyhow::{bail, Context};
use regex::Regex;
use std::path::{Path, PathBuf};

pub const DEFAULTS: &[&str] = &[".env*", "*.pem", "*.key", "secrets/"];

#[derive(Clone, Debug)]
pub struct NeverSent {
    patterns: Vec<String>,
    expressions: Vec<Regex>,
}
impl NeverSent {
    pub fn new(extra: &[String]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            extra.len() <= 128,
            "At most 128 custom never-sent patterns are supported"
        );
        let mut patterns: Vec<String> = DEFAULTS.iter().map(|s| (*s).into()).collect();
        for pattern in extra {
            validate(pattern)?;
            if !patterns.contains(pattern) {
                patterns.push(pattern.clone());
            }
        }
        let expressions = patterns
            .iter()
            .map(|p| Regex::new(&format!("(?i){}", expression(p))))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            patterns,
            expressions,
        })
    }
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
    pub fn matches(&self, relative: &Path) -> bool {
        let path = relative.to_string_lossy().replace('\\', "/");
        self.expressions.iter().any(|r| r.is_match(&path))
    }
    pub fn check(&self, root: &Path, path: &Path) -> anyhow::Result<()> {
        let relative = path
            .strip_prefix(root)
            .context("file is outside its project")?;
        if self.matches(relative) {
            bail!("File is excluded by the project's never-sent rules");
        }
        Ok(())
    }
    pub fn absolute_expressions(&self, root: &Path) -> Vec<String> {
        self.patterns
            .iter()
            .map(|p| {
                let relative = expression(p);
                format!(
                    "^{}{}",
                    case_literal(&format!("{}/", root.display())),
                    &relative[1..]
                )
            })
            .collect()
    }
    /// Freeze aliases of existing protected entries as well as matching future names.
    pub fn protected_targets(&self, root: &Path) -> anyhow::Result<Vec<PathBuf>> {
        fn walk(
            policy: &NeverSent,
            root: &Path,
            dir: &Path,
            out: &mut Vec<PathBuf>,
        ) -> anyhow::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                let metadata = std::fs::symlink_metadata(&path)?;
                if policy.matches(path.strip_prefix(root)?) {
                    if let Ok(target) = path.canonicalize() {
                        out.push(target);
                    }
                }
                if metadata.is_dir() && entry.file_name() != ".git" {
                    walk(policy, root, &path, out)?;
                }
            }
            Ok(())
        }
        let mut out = Vec::new();
        walk(self, root, root, &mut out)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let inodes: std::collections::HashSet<_> = out
                .iter()
                .filter_map(|p| std::fs::metadata(p).ok())
                .filter(|m| m.is_file())
                .map(|m| (m.dev(), m.ino()))
                .collect();
            fn aliases(
                dir: &Path,
                ids: &std::collections::HashSet<(u64, u64)>,
                out: &mut Vec<PathBuf>,
            ) -> anyhow::Result<()> {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    let metadata = std::fs::symlink_metadata(&path)?;
                    if metadata.is_file() && ids.contains(&(metadata.dev(), metadata.ino())) {
                        out.push(path.canonicalize()?);
                    }
                    if metadata.is_dir() && entry.file_name() != ".git" {
                        aliases(&path, ids, out)?;
                    }
                }
                Ok(())
            }
            if !inodes.is_empty() {
                aliases(root, &inodes, &mut out)?;
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}
pub fn validate(pattern: &str) -> anyhow::Result<()> {
    if pattern.is_empty()
        || pattern.len() > 512
        || pattern.starts_with('/')
        || pattern.contains(['\\', '\0', '\n', '\r', ':', '[', ']'])
        || pattern.split('/').any(|p| matches!(p, "." | ".."))
    {
        bail!("Use a relative never-sent glob with *, ** or ?; parent paths and character classes are not supported");
    }
    Ok(())
}
fn case_literal(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase())
            } else {
                regex::escape(&c.to_string())
            }
        })
        .collect()
}
fn expression(pattern: &str) -> String {
    let directory = pattern.ends_with('/');
    let pattern = pattern.trim_end_matches('/');
    let mut result = if pattern.contains('/') {
        "^".to_string()
    } else {
        "^(.*/)?".to_string()
    };
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    result.push_str("(.*/)?");
                } else {
                    result.push_str(".*");
                }
            }
            '*' => result.push_str("[^/]*"),
            '?' => result.push_str("[^/]"),
            _ => result.push_str(&case_literal(&c.to_string())),
        }
    }
    // Protect matching directory contents too; a glob may match a directory name.
    let _ = directory;
    result.push_str("(/.*)?$");
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_project_globs_cover_nested_new_files_and_case_aliases() {
        let p = NeverSent::new(&["private/**/*.json".into()]).unwrap();
        for path in [
            ".env",
            "sub/.ENV.local",
            "tls/key.PEM",
            "a/secret.key",
            "nested/secrets/key",
            "private/a/b/config.json",
            "private/config.json",
        ] {
            assert!(p.matches(Path::new(path)), "{path}");
        }
        for path in ["src/main.rs", "environment.ts", "private/config.rs"] {
            assert!(!p.matches(Path::new(path)));
        }
        for pattern in [
            "../private",
            "/etc",
            "x/../key",
            "a\\b",
            "x\n(allow default)",
            "[abc]",
        ] {
            assert!(NeverSent::new(&[pattern.into()]).is_err());
        }
    }
}
