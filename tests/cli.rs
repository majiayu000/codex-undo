#![cfg(unix)]
use serde_json::json;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_codex-undo")
}
#[test]
fn install_twice_uninstall_preserves_foreign_hooks_and_backups() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("hooks.json");
    let foreign = json!({"description":"existing","extra":{"keep":42},"hooks":{"Stop":[{"matcher":"custom","hooks":[{"type":"command","command":"echo foreign"}]},{"hooks":[]}],"SessionStart":[{"hooks":[{"type":"command","command":"echo remem"}]}]}});
    fs::write(&p, serde_json::to_vec_pretty(&foreign).unwrap()).unwrap();
    for _ in 0..2 {
        let out = Command::new(binary())
            .args(["install", "--hooks-file"])
            .arg(&p)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let doc: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(doc["extra"], foreign["extra"]);
        assert_eq!(doc["hooks"]["Stop"].as_array().unwrap().len(), 3);
        assert_eq!(doc["hooks"]["Stop"][0], foreign["hooks"]["Stop"][0]);
        assert_eq!(doc["hooks"]["Stop"][1], foreign["hooks"]["Stop"][1]);
        assert_eq!(doc["hooks"]["Interrupt"][0]["hooks"][0]["timeout"], 3);
        assert!(
            doc["hooks"]["PreToolUse"][0]
                .get("codexUndoManaged")
                .is_none()
        );
    }
    let out = Command::new(binary())
        .args(["uninstall", "--hooks-file"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(out.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    assert_eq!(doc["hooks"]["Stop"], foreign["hooks"]["Stop"]);
    assert_eq!(
        doc["hooks"]["SessionStart"],
        foreign["hooks"]["SessionStart"]
    );
    assert_eq!(doc["extra"], foreign["extra"]);
    assert_eq!(
        fs::read_dir(tmp.path())
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("backup-"))
            .count(),
        3
    );
}
#[test]
fn invalid_configuration_is_left_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("hooks.json");
    fs::write(&p, b"not json").unwrap();
    let out = Command::new(binary())
        .args(["install", "--hooks-file"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read(&p).unwrap(), b"not json");
}
#[test]
fn malformed_hook_always_exits_zero_and_reports_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("store");
    let mut child = Command::new(binary())
        .arg("--data-dir")
        .arg(&data)
        .args(["hook", "PreToolUse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"malformed scratch input")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "{}");
    assert!(data.join("hook-failures.log").exists());
    let out = Command::new(binary())
        .arg("--data-dir")
        .arg(&data)
        .args(["status", "--all"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("hook failure"));
}
#[test]
fn unknown_hook_fields_are_forward_tolerated_without_decisions() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    let data = cwd.join("store");
    let mut child = Command::new(binary())
        .arg("--data-dir")
        .arg(&data)
        .args(["hook", "SessionStart"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            serde_json::to_string(
                &json!({"session_id":"s","cwd":cwd,"hook_event_name":"SessionStart","unknown":42}),
            )
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "{}");
}

#[test]
fn hook_binary_handles_external_ignore_and_git_subdirectory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let cwd = root.join("repo");
    fs::create_dir(&cwd).unwrap();
    fs::create_dir(cwd.join("sub")).unwrap();
    fs::write(cwd.join("sibling.txt"), b"sibling").unwrap();
    fs::write(root.join("outside.txt"), b"outside").unwrap();
    fs::write(root.join("private.key"), b"private").unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&cwd)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&cwd)
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    let cwd = cwd.join("sub");
    fs::write(cwd.join(".codexundoignore"), b"*.key\n").unwrap();
    let data = root.join("store");
    for (event, extra) in [
        ("UserPromptSubmit", json!({"prompt":"regression"})),
        (
            "PreToolUse",
            json!({"tool_name":"apply_patch","tool_input":{"command":"*** Begin Patch\n*** Update File: ../../outside.txt\n*** Update File: ../../private.key\n*** End Patch"}}),
        ),
    ] {
        let mut v = json!({"session_id":"s","cwd":cwd,"hook_event_name":event,"turn_id":"t"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut child = Command::new(binary())
            .arg("--data-dir")
            .arg(&data)
            .args(["hook", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(serde_json::to_string(&v).unwrap().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "{}");
    }
    let store = codex_undo::Store::open(data.clone()).unwrap();
    let s = store.load("s").unwrap();
    assert_eq!(s.turns.len(), 1);
    assert!(s.tracked.contains(&root.join("outside.txt")));
    assert!(s.tracked.contains(&root.join("repo/sibling.txt")));
    assert!(!s.tracked.contains(&root.join("private.key")));
    assert!(!data.join("hook-failures.log").exists());
}
