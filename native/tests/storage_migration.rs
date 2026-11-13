use gitrust_desktop::storage::{self, Preferences};

#[test]
fn preferences_roundtrip_is_bounded_deduplicated_and_backed_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config/preferences.json");
    let mut preferences = Preferences::default();
    for n in 0..30 {
        preferences.remember(format!("/tmp/project-{n}").into());
    }
    preferences.remember("/tmp/project-20".into());
    assert_eq!(preferences.recent.len(), 20);
    storage::save(&path, &preferences).unwrap();
    let loaded = storage::load(&path).unwrap();
    assert_eq!(loaded.recent, preferences.recent);
    preferences.remember("/tmp/next".into());
    storage::save(&path, &preferences).unwrap();
    assert_eq!(
        storage::load(&path.with_extension("json.bak"))
            .unwrap()
            .recent,
        loaded.recent
    );
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        2
    );
}

#[test]
fn future_schema_corrupt_and_oversized_preferences_are_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.json");
    for bytes in [
        br#"{"schema_version":42,"recent":[]}"#.to_vec(),
        b"{broken".to_vec(),
        vec![b'x'; 70_000],
    ] {
        std::fs::write(&path, &bytes).unwrap();
        assert!(storage::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
