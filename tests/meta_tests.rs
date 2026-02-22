use idio_loom::config::Config;
use idio_loom::meta::Meta;
use std::fs;
use tempfile::TempDir;

fn setup() -> (TempDir, Config) {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("loom.yaml"), "model: sonnet\n").unwrap();
    fs::create_dir_all(tmp.path().join("agents")).unwrap();
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    (tmp, config)
}

#[test]
fn new_meta() {
    let meta = Meta::new("test", true);
    assert_eq!(meta.name, "test");
    assert!(meta.snapshots);
    assert!(meta.thread.is_empty());
    assert!(meta.agents.is_empty());
    assert!(meta.running.is_none());
}

#[test]
fn next_turn_number_empty() {
    let meta = Meta::new("test", true);
    assert_eq!(meta.next_turn_number(), 0);
}

#[test]
fn ensure_agent_creates() {
    let mut meta = Meta::new("test", true);
    let agent = meta.ensure_agent("axe");
    assert!(agent.session_id.is_none());
}

#[test]
fn ensure_agent_idempotent() {
    let mut meta = Meta::new("test", true);
    meta.ensure_agent("axe").session_id = Some("sess-123".into());
    // Second call should not overwrite
    let agent = meta.ensure_agent("axe");
    assert_eq!(agent.session_id.as_deref(), Some("sess-123"));
}

#[test]
fn last_turn_for_agent_found() {
    let mut meta = Meta::new("test", true);
    meta.thread.push(idio_loom::meta::Turn {
        turn: 0,
        agent: "axe".into(),
        source: None,
        nudge: None,
        stage: None,
        model: None,
        chars: 100,
        elapsed_s: 1.0,
        cost_usd: None,
    });
    meta.thread.push(idio_loom::meta::Turn {
        turn: 1,
        agent: "bobby".into(),
        source: None,
        nudge: None,
        stage: None,
        model: None,
        chars: 200,
        elapsed_s: 2.0,
        cost_usd: None,
    });
    let found = meta.last_turn_for_agent("axe").unwrap();
    assert_eq!(found.turn, 0);
}

#[test]
fn last_turn_for_agent_missing() {
    let meta = Meta::new("test", true);
    assert!(meta.last_turn_for_agent("ghost").is_none());
}

#[test]
fn save_read_roundtrip() {
    let (_tmp, config) = setup();
    let mut meta = Meta::new("roundtrip", true);
    meta.ensure_agent("axe").session_id = Some("sess-abc".into());
    meta.thread.push(idio_loom::meta::Turn {
        turn: 0,
        agent: "axe".into(),
        source: None,
        nudge: Some("do it".into()),
        stage: None,
        model: Some("sonnet".into()),
        chars: 50,
        elapsed_s: 1.5,
        cost_usd: Some(0.01),
    });
    meta.save(&config).unwrap();

    let loaded = Meta::read(&config, "roundtrip").unwrap();
    assert_eq!(loaded.name, "roundtrip");
    assert_eq!(loaded.thread.len(), 1);
    assert_eq!(loaded.thread[0].nudge.as_deref(), Some("do it"));
    assert_eq!(
        loaded.agents.get("axe").unwrap().session_id.as_deref(),
        Some("sess-abc")
    );
}

#[test]
fn read_missing_bails() {
    let (_tmp, config) = setup();
    assert!(Meta::read(&config, "nonexistent").is_err());
}

#[test]
fn read_or_create_missing_returns_fresh() {
    let (_tmp, config) = setup();
    let meta = Meta::read_or_create(&config, "fresh", true).unwrap();
    assert_eq!(meta.name, "fresh");
    assert!(meta.thread.is_empty());
}

#[test]
fn read_or_create_existing_reads() {
    let (_tmp, config) = setup();
    let mut original = Meta::new("existing", false);
    original.ensure_agent("axe");
    original.save(&config).unwrap();

    let loaded = Meta::read_or_create(&config, "existing", true).unwrap();
    assert!(loaded.agents.contains_key("axe"));
    // snapshots comes from the file, not the parameter
    assert!(!loaded.snapshots);
}
