use idio_loom::config::Config;
use idio_loom::loom::build_message;
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
fn nudge_only() {
    let (_tmp, config) = setup();
    let meta = Meta::new("test", true);
    let msg = build_message(&config, "test", &meta, "axe", None, Some("research AAPL")).unwrap();
    assert_eq!(msg, "research AAPL");
}

#[test]
fn source_agent() {
    let (_tmp, config) = setup();
    let mut meta = Meta::new("test", true);
    // Create a turn file for the source agent
    let run_dir = Meta::run_dir(&config, "test");
    fs::create_dir_all(&run_dir).unwrap();
    meta.thread.push(idio_loom::meta::Turn {
        turn: 0,
        agent: "axe".into(),
        source: None,
        nudge: None,
        stage: None,
        model: None,
        chars: 5,
        elapsed_s: 1.0,
        cost_usd: None,
    });
    let turn_file = Meta::turn_file(&config, "test", 0, "axe");
    fs::write(&turn_file, "axe output here").unwrap();

    let msg = build_message(&config, "test", &meta, "bobby", Some("axe"), None).unwrap();
    assert_eq!(msg, "axe output here");
}

#[test]
fn source_all() {
    let (_tmp, config) = setup();
    let mut meta = Meta::new("test", true);
    let run_dir = Meta::run_dir(&config, "test");
    fs::create_dir_all(&run_dir).unwrap();

    meta.thread.push(idio_loom::meta::Turn {
        turn: 0,
        agent: "axe".into(),
        source: None,
        nudge: Some("go".into()),
        stage: None,
        model: None,
        chars: 3,
        elapsed_s: 1.0,
        cost_usd: None,
    });
    fs::write(Meta::turn_file(&config, "test", 0, "axe"), "AAA").unwrap();

    // bobby hears all — should see axe's turn but not own turns
    let msg = build_message(&config, "test", &meta, "bobby", Some("all"), None).unwrap();
    assert!(msg.contains("[axe]:"));
    assert!(msg.contains("AAA"));
    assert!(msg.contains("[user → axe]: go"));
}

#[test]
fn nudge_plus_source_has_separator() {
    let (_tmp, config) = setup();
    let mut meta = Meta::new("test", true);
    let run_dir = Meta::run_dir(&config, "test");
    fs::create_dir_all(&run_dir).unwrap();

    meta.thread.push(idio_loom::meta::Turn {
        turn: 0,
        agent: "axe".into(),
        source: None,
        nudge: None,
        stage: None,
        model: None,
        chars: 5,
        elapsed_s: 1.0,
        cost_usd: None,
    });
    fs::write(Meta::turn_file(&config, "test", 0, "axe"), "hello").unwrap();

    let msg =
        build_message(&config, "test", &meta, "bobby", Some("axe"), Some("focus on risk")).unwrap();
    assert!(msg.contains("hello"));
    assert!(msg.contains("---"));
    assert!(msg.contains("focus on risk"));
}

#[test]
fn empty_returns_empty() {
    let (_tmp, config) = setup();
    let meta = Meta::new("test", true);
    let msg = build_message(&config, "test", &meta, "axe", None, None).unwrap();
    assert!(msg.is_empty());
}
