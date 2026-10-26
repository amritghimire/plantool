use plantool_core::Provider;
use std::ffi::{OsStr, OsString};

/// Rebuild the child environment. In particular, loader hooks and shell startup
/// files must not execute inside sandbox-exec before the profile is installed.
pub fn filtered(
    provider: Option<Provider>,
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(key, _)| allowed(provider, key))
        .collect()
}
fn allowed(provider: Option<Provider>, key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    let key = key.to_ascii_uppercase();
    if key == "GH_TOKEN"
        || key.starts_with("GITHUB_")
        || key.starts_with("GH_")
        || key == "SSH_AUTH_SOCK"
    {
        return false;
    }
    if key.ends_with("_API_KEY") {
        return provider == Some(Provider::Claude) && key == "ANTHROPIC_API_KEY";
    }
    matches!(
        key.as_str(),
        "PATH"
            | "HOME"
            | "USERPROFILE"
            | "SYSTEMROOT"
            | "WINDIR"
            | "COMSPEC"
            | "PATHEXT"
            | "TEMP"
            | "TMP"
            | "TMPDIR"
            | "USER"
            | "USERNAME"
            | "LOGNAME"
            | "SHELL"
            | "LANG"
            | "TERM"
            | "TZ"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "ALL_PROXY"
            | "NO_PROXY"
            | "CODEX_CA_CERTIFICATE"
            | "SSL_CERT_FILE"
            | "SSL_CERT_DIR"
            | "CURL_CA_BUNDLE"
            | "REQUESTS_CA_BUNDLE"
            | "NODE_EXTRA_CA_CERTS"
            | "XDG_CONFIG_HOME"
            | "XDG_CACHE_HOME"
            | "XDG_DATA_HOME"
            | "CODEX_HOME"
            | "CLAUDE_CONFIG_DIR"
            | "COPILOT_HOME"
            | "OLLAMA_HOST"
            | "ANTHROPIC_BASE_URL"
            | "NIX_PATH"
            | "NIX_PROFILES"
            | "NIX_SSL_CERT_FILE"
    ) || key.starts_with("LC_")
}
pub fn apply(command: &mut tokio::process::Command, provider: Option<Provider>) {
    command
        .env_clear()
        .envs(filtered(provider, std::env::vars_os()));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_and_loader_injection_are_removed_but_needed_model_auth_is_preserved() {
        let vars = [
            "PATH",
            "HOME",
            "HTTPS_PROXY",
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "SSH_AUTH_SOCK",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "OTHER_API_KEY",
            "BASH_ENV",
            "DYLD_INSERT_LIBRARIES",
            "LD_PRELOAD",
            "GIT_CONFIG_COUNT",
        ];
        let input = || {
            vars.iter()
                .map(|k| (OsString::from(k), OsString::from("fixture")))
        };
        let claude = filtered(Some(Provider::Claude), input());
        assert_eq!(claude.len(), 4);
        let codex = filtered(Some(Provider::Codex), input());
        assert_eq!(codex.len(), 3);
        assert_eq!(filtered(None, input()).len(), 3);
    }
}
