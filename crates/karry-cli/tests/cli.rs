use std::process::Command;

fn run(home: &std::path::Path, args: &[&str]) -> (i32, serde_json::Value) {
    let o = Command::new(env!("CARGO_BIN_EXE_karry-cli")).arg("--json").args(args).env("HOME", home).env_remove("XDG_CONFIG_HOME").env_remove("XDG_DATA_HOME").env_remove("XDG_STATE_HOME").output().unwrap();
    (o.status.code().unwrap(), serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("not json: {}", String::from_utf8_lossy(&o.stdout))))
}

#[test]
fn json_commands_local_export_restore_uninstall() {
    let t = tempfile::tempdir().unwrap();
    let h = t.path();
    std::fs::create_dir_all(h.join(".config")).unwrap();
    std::fs::write(h.join(".config/kwinrc"), "[Windows]\nBorderSnapZone=10\n").unwrap();
    assert_eq!(run(h, &["init", "--local"]).1["path"], h.join("Karry").to_str().unwrap());
    assert_eq!(run(h, &["info"]).1["local_only"], true);
    assert!(run(h, &["backup"]).1["shared"].as_u64().unwrap() >= 1);
    let f = h.join("a.karry");
    assert_eq!(run(h, &["export", f.to_str().unwrap()]).0, 0);
    assert!(run(h, &["restore", f.to_str().unwrap(), "--dry-run"]).1["plan"]["changes"].is_array());
    assert!(run(h, &["sync", "--dry-run", "--remote-wins", "[]"]).1["conflicts"].is_array());
    let (code, e) = run(h, &["fix", "nope", "nope"]);
    assert_eq!((code, e["error"].is_string()), (1, true));
    assert_eq!(run(h, &["uninstall", "--yes"]).0, 0);
    assert!(!h.join(".config/karry").exists() && h.join(".config/kwinrc").exists() && h.join("Karry").exists());
}
