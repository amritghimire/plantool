//! Background credential reads never wait for a hidden macOS authorization dialog.
//! Only explicit owner-authorized actions may opt into the interactive path.
#[cfg(target_os = "macos")]
pub(crate) fn read(
    service: &str,
    account: Option<&str>,
    interactive: bool,
) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context;
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    use std::sync::{Mutex, OnceLock};

    // SecKeychainSetUserInteractionAllowed is process global. Serialize every
    // Keychain query so a background read cannot suppress an explicitly
    // authorized interactive read (or vice versa).
    static KEYCHAIN_QUERY: OnceLock<Mutex<()>> = OnceLock::new();
    let _query_guard = KEYCHAIN_QUERY
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _interaction_guard = if interactive {
        None
    } else {
        Some(
            security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()
                .context("Could not disable Keychain prompts for background work")?,
        )
    };
    let mut query = ItemSearchOptions::new();
    query
        .class(ItemClass::generic_password())
        .service(service)
        .load_data(true)
        .skip_authenticated_items(!interactive);
    if let Some(account) = account {
        query.account(account);
    }
    let rows = query.search().with_context(|| {
        format!("Keychain credential unavailable for {service}. Unlock Keychain and authorize the current PlanTool binary; background work will retry without prompting.")
    })?;
    rows.into_iter()
        .find_map(|r| match r {
            SearchResult::Data(data) => Some(data),
            _ => None,
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Keychain credential missing or requires authorization for {service}")
        })
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn read(
    _service: &str,
    _account: Option<&str>,
    _interactive: bool,
) -> anyhow::Result<Vec<u8>> {
    anyhow::bail!("Provider keys require macOS Keychain")
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn a_missing_credential_returns_without_an_authentication_prompt() {
        let started = std::time::Instant::now();
        assert!(super::read(
            &format!("plantool-missing-test/{}", uuid::Uuid::new_v4()),
            None,
            false
        )
        .is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
