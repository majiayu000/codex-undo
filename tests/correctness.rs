#![cfg(unix)]
use codex_undo::*;
use proptest::prelude::*;
use serde_json::json;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

struct Fixture {
    _tmp: tempfile::TempDir,
    cwd: std::path::PathBuf,
    store: Store,
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        fs::create_dir(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let store = Store::open(tmp.path().join("store")).unwrap();
        Self {
            _tmp: tmp,
            cwd,
            store,
        }
    }
    fn write(&self, path: &str, value: &[u8]) {
        fs::write(self.cwd.join(path), value).unwrap();
    }
    fn event(&mut self, event: &str, turn: usize, extra: serde_json::Value) {
        let mut v = json!({"session_id":"test-session","cwd":self.cwd,"hook_event_name":event,"turn_id":format!("turn-{turn}"),"prompt":format!("turn {turn}")});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.store.hook(&v).unwrap();
    }
    fn start(&mut self, n: usize) {
        self.event("UserPromptSubmit", n, json!({}));
    }
    fn stop(&mut self, n: usize) {
        self.event("Stop", n, json!({}));
    }
    fn patch(&mut self, n: usize, path: &str) {
        self.event("PreToolUse",n,json!({"tool_name":"apply_patch","tool_input":{"command":format!("*** Begin Patch\n*** Update File: {path}\n*** End Patch")}}));
    }
    fn restore(&mut self, target: &str, force: bool) -> usize {
        let mut s = self.store.load("test-session").unwrap();
        let plan = self.store.plan(&s, target).unwrap();
        self.store.restore(&mut s, target, force, &plan).unwrap()
    }
    fn target(&self, n: usize) -> String {
        self.store.load("test-session").unwrap().turns[n - 1]
            .start
            .clone()
    }
}
#[test]
fn untracked_delete_restore_redo_and_idempotence() {
    let mut f = Fixture::new();
    f.write("notes.txt", b"original");
    f.start(1);
    fs::remove_file(f.cwd.join("notes.txt")).unwrap();
    f.stop(1);
    let target = f.target(1);
    assert_eq!(f.restore(&target, false), 1);
    assert_eq!(fs::read(f.cwd.join("notes.txt")).unwrap(), b"original");
    assert_eq!(f.restore(&target, false), 0);
    let safety = f
        .store
        .load("test-session")
        .unwrap()
        .restore
        .unwrap()
        .safety;
    f.restore(&safety, false);
    assert!(!f.cwd.join("notes.txt").exists());
}
#[test]
fn explicit_absence_deletes_new_patch_file_but_missing_entry_does_not() {
    let mut f = Fixture::new();
    f.start(1);
    f.patch(1, "new.txt");
    f.write("new.txt", b"agent");
    f.write("opaque.txt", b"shell-created");
    f.stop(1);
    f.restore(&f.target(1), false);
    assert!(!f.cwd.join("new.txt").exists());
    assert_eq!(
        fs::read(f.cwd.join("opaque.txt")).unwrap(),
        b"shell-created"
    );
}
#[test]
fn unknown_paths_and_git_metadata_are_untouched() {
    let mut f = Fixture::new();
    f.write("tracked.txt", b"before");
    f.start(1);
    f.write("tracked.txt", b"after");
    f.stop(1);
    f.write(".private", b"manual");
    fs::create_dir(f.cwd.join(".git")).unwrap();
    f.write(".git/config", b"git-state");
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(f.cwd.join(".private")).unwrap(), b"manual");
    assert_eq!(fs::read(f.cwd.join(".git/config")).unwrap(), b"git-state");
    assert!(f.store.safe_path(&f.cwd.join(".git/config")).is_err());
}
#[test]
fn ignore_rules_apply_at_restore_and_redo() {
    let mut f = Fixture::new();
    f.write("ignored.txt", b"before");
    f.write("normal.txt", b"before");
    f.start(1);
    f.write("ignored.txt", b"after");
    f.write("normal.txt", b"after");
    f.stop(1);
    f.write(".codexundoignore", b"ignored.txt\n");
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(f.cwd.join("ignored.txt")).unwrap(), b"after");
    let safety = f
        .store
        .load("test-session")
        .unwrap()
        .restore
        .unwrap()
        .safety;
    f.write("ignored.txt", b"later");
    f.restore(&safety, false);
    assert_eq!(fs::read(f.cwd.join("ignored.txt")).unwrap(), b"later");
}
#[test]
fn ignored_patch_paths_never_enter_manifests() {
    let mut f = Fixture::new();
    f.write(".codexundoignore", b"secret.txt\n");
    f.start(1);
    f.patch(1, "secret.txt");
    f.write("secret.txt", b"private");
    f.stop(1);
    let s = f.store.load("test-session").unwrap();
    assert!(!s.tracked.contains(&f.cwd.join("secret.txt")));
    assert!(
        !f.store
            .manifest(&s.turns[0].start)
            .unwrap()
            .contains_key(&f.cwd.join("secret.txt"))
    );
}
#[test]
fn hidden_paths_are_only_tracked_when_explicitly_edited() {
    let mut f = Fixture::new();
    f.write(".env", b"before");
    f.start(1);
    assert!(
        !f.store
            .manifest(&f.target(1))
            .unwrap()
            .contains_key(&f.cwd.join(".env"))
    );
    f.patch(1, ".env");
    f.write(".env", b"after");
    f.stop(1);
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(f.cwd.join(".env")).unwrap(), b"before");
}
#[test]
fn default_conflict_refuses_all_changes_force_keeps_safety() {
    let mut f = Fixture::new();
    f.write("a", b"before");
    f.write("b", b"before");
    f.start(1);
    f.write("a", b"agent");
    f.write("b", b"agent");
    f.stop(1);
    f.write("a", b"user");
    let mut s = f.store.load("test-session").unwrap();
    let target = f.target(1);
    let plan = f.store.plan(&s, &target).unwrap();
    assert!(plan.iter().any(|c| c.conflict));
    assert!(f.store.restore(&mut s, &target, false, &plan).is_err());
    assert_eq!(fs::read(f.cwd.join("b")).unwrap(), b"agent");
    f.restore(&target, true);
    let safety = f
        .store
        .load("test-session")
        .unwrap()
        .restore
        .unwrap()
        .safety;
    f.restore(&safety, false);
    assert_eq!(fs::read(f.cwd.join("a")).unwrap(), b"user");
}
#[test]
fn edit_after_preview_refuses_even_force() {
    let mut f = Fixture::new();
    f.write("a", b"before");
    f.start(1);
    f.write("a", b"agent");
    f.stop(1);
    let mut s = f.store.load("test-session").unwrap();
    let target = f.target(1);
    let plan = f.store.plan(&s, &target).unwrap();
    f.write("a", b"concurrent");
    assert!(f.store.restore(&mut s, &target, true, &plan).is_err());
    assert_eq!(fs::read(f.cwd.join("a")).unwrap(), b"concurrent");
}
#[test]
fn symlink_hardlink_and_symlink_parent_are_never_restored() {
    let mut f = Fixture::new();
    f.write("a", b"before");
    f.start(1);
    f.write("a", b"after");
    f.stop(1);
    let external = f._tmp.path().join("external");
    fs::write(&external, b"external").unwrap();
    fs::remove_file(f.cwd.join("a")).unwrap();
    symlink(&external, f.cwd.join("a")).unwrap();
    let s = f.store.load("test-session").unwrap();
    assert!(f.store.plan(&s, &f.target(1)).is_err());
    assert_eq!(fs::read(&external).unwrap(), b"external");
    fs::remove_file(f.cwd.join("a")).unwrap();
    fs::hard_link(&external, f.cwd.join("a")).unwrap();
    assert!(f.store.plan(&s, &f.target(1)).is_err());
    symlink(f._tmp.path(), f.cwd.join("linkdir")).unwrap();
    assert!(f.store.safe_path(&f.cwd.join("linkdir/absent")).is_err());
}
#[test]
fn permissions_and_binary_bytes_survive_restore() {
    let mut f = Fixture::new();
    f.write("bin", &[0, 255, 42]);
    fs::set_permissions(f.cwd.join("bin"), fs::Permissions::from_mode(0o751)).unwrap();
    f.start(1);
    f.write("bin", b"after");
    fs::set_permissions(f.cwd.join("bin"), fs::Permissions::from_mode(0o600)).unwrap();
    f.stop(1);
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(f.cwd.join("bin")).unwrap(), [0, 255, 42]);
    assert_eq!(
        fs::metadata(f.cwd.join("bin"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
}
#[test]
fn tracking_is_monotonic_and_rewind_keeps_late_unobserved_paths() {
    let mut f = Fixture::new();
    f.write("a", b"one");
    f.start(1);
    f.write("a", b"two");
    f.stop(1);
    f.start(2);
    f.patch(2, "new");
    f.write("new", b"new");
    f.stop(2);
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(f.cwd.join("new")).unwrap(), b"new");
    assert!(
        f.store
            .load("test-session")
            .unwrap()
            .tracked
            .contains(&f.cwd.join("new"))
    );
}
#[test]
fn interrupted_hook_records_checkpoint_and_notice_is_consumed_once() {
    let mut f = Fixture::new();
    f.write("a", b"before");
    f.start(1);
    f.write("a", b"after");
    f.event("Interrupt", 1, json!({}));
    assert!(f.store.load("test-session").unwrap().turns[0].interrupted);
    f.restore(&f.target(1), false);
    let v = json!({"session_id":"test-session","cwd":f.cwd,"hook_event_name":"UserPromptSubmit","turn_id":"turn-2","prompt":"continue"});
    let out = f.store.hook(&v).unwrap();
    assert!(
        out["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("Re-read")
    );
    f.stop(2);
    f.event("UserPromptSubmit", 3, json!({}));
    assert!(f.store.load("test-session").unwrap().notice.is_none());
}
#[test]
fn corrupt_blob_blocks_restore_before_any_mutation() {
    let mut f = Fixture::new();
    f.write("a", b"before");
    f.start(1);
    f.write("a", b"after");
    f.stop(1);
    let m = f.store.manifest(&f.target(1)).unwrap();
    if let Entry::File { blob, .. } = &m[&f.cwd.join("a")] {
        fs::write(
            f.store.root.join("blobs").join(&blob[..2]).join(blob),
            b"corrupt",
        )
        .unwrap();
    }
    let s = f.store.load("test-session").unwrap();
    let result = f.store.plan(&s, &f.target(1));
    if let Ok(plan) = result {
        let mut s = s;
        assert!(f.store.restore(&mut s, &f.target(1), false, &plan).is_err());
    }
    assert_eq!(fs::read(f.cwd.join("a")).unwrap(), b"after");
}
#[test]
fn patch_move_tracks_both_old_and_new_paths() {
    let cwd = Path::new("/tmp/project");
    let paths=tool_paths(&json!({"tool_name":"apply_patch","tool_input":{"command":"*** Begin Patch\n*** Update File: old\n*** Move to: new\n*** End Patch"}}),cwd).unwrap();
    assert!(paths.contains(&cwd.join("old")));
    assert!(paths.contains(&cwd.join("new")));
}
#[test]
fn no_prompt_is_explicit_gap_and_unknown_fields_are_ignored() {
    let mut f = Fixture::new();
    f.event("PreToolUse", 1, json!({"new_field":42,"tool_name":"Bash"}));
    let s = f.store.load("test-session").unwrap();
    assert!(s.turns.is_empty());
    assert!(!s.gaps.is_empty());
}
#[test]
fn gc_retains_redo_and_collects_only_orphans() {
    let mut f = Fixture::new();
    f.write("a", b"before");
    f.start(1);
    f.write("a", b"after");
    f.stop(1);
    f.restore(&f.target(1), false);
    f.write("orphan", b"orphan");
    f.store.capture(&f.cwd.join("orphan"), false).unwrap();
    assert!(f.store.gc().unwrap().0 >= 1);
    let safety = f
        .store
        .load("test-session")
        .unwrap()
        .restore
        .unwrap()
        .safety;
    f.restore(&safety, false);
    assert_eq!(fs::read(f.cwd.join("a")).unwrap(), b"after");
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_turn_sequences_rewind_redo_and_ignore_invariants(ops in prop::collection::vec((0usize..4,prop::collection::vec(any::<u8>(),0..40)),1..16)) {
        let mut f=Fixture::new();f.write(".codexundoignore",b"ignored\n");f.write("ignored",b"untouched");f.write(".unknown",b"untouched");
        for i in 0..4 {f.write(&format!("file-{i}"),b"initial");}
        for (n,(idx,bytes)) in ops.iter().enumerate() {
            f.start(n+1);let p=format!("file-{idx}");f.patch(n+1,&p);
            if bytes.is_empty() {let _=fs::remove_file(f.cwd.join(&p));} else {f.write(&p,bytes);}
            f.stop(n+1);
        }
        let before=(0..4).map(|i|fs::read(f.cwd.join(format!("file-{i}"))).ok()).collect::<Vec<_>>();
        let target=f.target(1);f.restore(&target,false);prop_assert_eq!(f.restore(&target,false),0);
        let safety=f.store.load("test-session").unwrap().restore.unwrap().safety;f.restore(&safety,false);
        let after=(0..4).map(|i|fs::read(f.cwd.join(format!("file-{i}"))).ok()).collect::<Vec<_>>();
        prop_assert_eq!(before,after);prop_assert_eq!(fs::read(f.cwd.join("ignored")).unwrap(),b"untouched");prop_assert_eq!(fs::read(f.cwd.join(".unknown")).unwrap(),b"untouched");
    }
}

#[test]
fn external_patch_path_uses_basename_ignore_without_panic() {
    let mut f = Fixture::new();
    f.write(".codexundoignore", b"*.key\n");
    let outside = f.cwd.parent().unwrap().join("outside.txt");
    fs::write(&outside, b"before").unwrap();
    f.start(1);
    f.patch(1, "../outside.txt");
    fs::write(&outside, b"after").unwrap();
    f.stop(1);
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(&outside).unwrap(), b"before");
    let safety = f
        .store
        .load("test-session")
        .unwrap()
        .restore
        .unwrap()
        .safety;
    f.restore(&safety, false);
    assert_eq!(fs::read(&outside).unwrap(), b"after");
}
#[test]
fn ignored_external_key_is_never_captured_or_restored() {
    let mut f = Fixture::new();
    f.write(".codexundoignore", b"*.key\n");
    let outside = f.cwd.parent().unwrap().join("private.key");
    fs::write(&outside, b"before").unwrap();
    f.start(1);
    f.patch(1, "../private.key");
    fs::write(&outside, b"after").unwrap();
    f.stop(1);
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(&outside).unwrap(), b"after");
    assert!(
        !f.store
            .load("test-session")
            .unwrap()
            .tracked
            .contains(&outside)
    );
}
#[test]
fn git_subdirectory_index_siblings_do_not_panic_or_change_git() {
    use std::process::Command;
    let mut f = Fixture::new();
    f.write("sibling.txt", b"before");
    f.write("sibling.key", b"private");
    fs::create_dir(f.cwd.join("sub")).unwrap();
    f.write("sub/file.txt", b"inside");
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&f.cwd)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&f.cwd)
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    let git = walkdir::WalkDir::new(f.cwd.join(".git"))
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().to_path_buf(), fs::read(e.path()).unwrap()))
        .collect::<Vec<_>>();
    f.cwd = f.cwd.join("sub");
    f.write(".codexundoignore", b"*.key\n");
    f.start(1);
    let s = f.store.load("test-session").unwrap();
    assert!(
        s.tracked
            .contains(&f.cwd.parent().unwrap().join("sibling.txt"))
    );
    assert!(
        !s.tracked
            .contains(&f.cwd.parent().unwrap().join("sibling.key"))
    );
    for (p, b) in git {
        assert_eq!(fs::read(p).unwrap(), b);
    }
}

#[test]
fn restoring_ignore_file_uses_policy_from_operation_start() {
    let mut f = Fixture::new();
    f.write(".codexundoignore", b"normal\n");
    f.write("normal", b"before");
    f.start(1);
    f.patch(1, ".codexundoignore");
    f.write(".codexundoignore", b"# now track normal\n");
    f.patch(1, "normal");
    f.write("normal", b"after");
    f.stop(1);
    f.restore(&f.target(1), false);
    assert_eq!(fs::read(f.cwd.join("normal")).unwrap(), b"before");
    assert_eq!(
        fs::read(f.cwd.join(".codexundoignore")).unwrap(),
        b"normal\n"
    );
}
