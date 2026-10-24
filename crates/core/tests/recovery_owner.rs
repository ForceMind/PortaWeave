use portaweave_core::{Recovery, RecoveryLimits, Volume};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn snapshot_preserves_nonempty_lock_file_through_owning_handle() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pw-lock-copy-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let source = root.join("source");
    drop(Volume::create(&source, 100).unwrap());
    let bytes = b"diagnostic lock content must not be omitted or replaced";
    fs::write(source.join("session.lock"), bytes).unwrap();
    let workspace = root.join("evidence-work");
    let recovery = Recovery::prepare(&source, &workspace, RecoveryLimits::default()).unwrap();
    recovery.verify().unwrap();
    assert_eq!(fs::read(source.join("session.lock")).unwrap(), bytes);
    assert_eq!(
        fs::read(workspace.join("evidence/session.lock")).unwrap(),
        bytes
    );
    assert!(workspace.join("SNAPSHOT_COMPLETE").exists());
    assert!(!workspace.join("SNAPSHOT_INCOMPLETE").exists());
    drop(recovery);
    fs::remove_dir_all(root).unwrap();
}
