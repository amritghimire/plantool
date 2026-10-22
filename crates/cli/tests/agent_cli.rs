#[test]
fn hosted_and_sandbox_marked_processes_cannot_manage_the_daemon_or_enroll() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["setup"],
        vec!["serve"],
        vec!["daemon", "stop"],
        vec!["daemon", "restart", "--force"],
        vec!["daemon", "start"],
    ] {
        for token in [true, false] {
            let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_plantool"));
            cmd.args(&args)
                .env("PLANTOOL_HOME", home.path().join("unused"))
                .env("PLANTOOL_PORT", "41300");
            if token {
                cmd.env("PLANTOOL_RUN_TOKEN", "expired");
            } else {
                cmd.env_remove("PLANTOOL_RUN_TOKEN")
                    .env("PLANTOOL_SANDBOXED", "1");
            }
            let output = cmd.output().unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr)
                .contains("unavailable to agent/sandboxed runs"));
            assert!(!home.path().join("unused").exists());
        }
    }
}
