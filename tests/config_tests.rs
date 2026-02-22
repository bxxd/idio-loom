use idio_loom::config::Config;
use std::fs;
use tempfile::TempDir;

fn setup_workspace(yaml: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("loom.yaml"), yaml).unwrap();
    fs::create_dir_all(tmp.path().join("agents")).unwrap();
    tmp
}

fn write_agent(tmp: &TempDir, name: &str, yaml: &str) {
    fs::write(tmp.path().join("agents").join(format!("{}.yaml", name)), yaml).unwrap();
}

#[test]
fn workshop_dir_relative() {
    let tmp = setup_workspace("model: sonnet\nworkshop: .\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert_eq!(config.workshop_dir(), config.home.join("."));
}

#[test]
fn workshop_dir_absolute() {
    let tmp = setup_workspace("model: sonnet\nworkshop: /tmp/ws\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert_eq!(config.workshop_dir(), std::path::PathBuf::from("/tmp/ws"));
}

#[test]
fn state_dir_relative() {
    let tmp = setup_workspace("model: sonnet\nstate: .loom\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert_eq!(config.state_dir(), config.home.join(".loom"));
}

#[test]
fn state_dir_absolute() {
    let tmp = setup_workspace("model: sonnet\nstate: /tmp/state\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert_eq!(config.state_dir(), std::path::PathBuf::from("/tmp/state"));
}

#[test]
fn claude_cwd_defaults_to_home() {
    let tmp = setup_workspace("model: sonnet\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert_eq!(config.claude_cwd(), config.home);
}

#[test]
fn claude_cwd_absolute() {
    let other = TempDir::new().unwrap();
    let yaml = format!("model: sonnet\ncwd: {}\n", other.path().display());
    let tmp = setup_workspace(&yaml);
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    // canonicalize both for comparison
    assert_eq!(config.claude_cwd(), other.path().canonicalize().unwrap());
}

#[test]
fn agent_model_override() {
    let tmp = setup_workspace("model: sonnet\n");
    write_agent(&tmp, "fast", "model: haiku\nsystem_prompt: hi\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert_eq!(config.agent_model("fast"), "haiku");
}

#[test]
fn agent_model_fallback() {
    let tmp = setup_workspace("model: sonnet\n");
    write_agent(&tmp, "basic", "system_prompt: hi\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert_eq!(config.agent_model("basic"), "sonnet");
}

#[test]
fn require_agent_exists() {
    let tmp = setup_workspace("model: sonnet\n");
    write_agent(&tmp, "axe", "system_prompt: research\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert!(config.require_agent("axe").is_ok());
}

#[test]
fn require_agent_missing() {
    let tmp = setup_workspace("model: sonnet\n");
    let config = Config::load(Some(tmp.path().to_str().unwrap())).unwrap();
    assert!(config.require_agent("ghost").is_err());
}
