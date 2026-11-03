// Own test binary: it rewrites PATH and HOME for the whole process.
#[test]
fn missing_t3_is_hidden_without_breaking_discovery() {
    let empty = tempfile::tempdir().unwrap();
    std::env::set_var("PATH", empty.path());
    std::env::set_var("HOME", empty.path());
    std::env::remove_var("PLANTOOL_T3_PATH");
    let providers = plantool_daemon::providers::discover();
    assert!(providers.iter().all(|p| p.id != "t3"));
    assert!(providers.iter().any(|p| p.id == "claude"));
}
