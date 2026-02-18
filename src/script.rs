use anyhow::{bail, Result};

use crate::config::Config;
use crate::meta::Meta;

pub fn run_script(
    config: &Config,
    name: &str,
    script_path: &str,
    intake: Option<&str>,
    model: Option<&str>,
) -> Result<()> {
    let run_dir = Meta::run_dir(config, name);
    std::fs::create_dir_all(&run_dir)?;

    let mut cmd = std::process::Command::new("bash");
    cmd.arg(script_path);
    cmd.env("LOOM_NAME", name);
    cmd.env("LOOM_INTAKE", intake.unwrap_or(""));
    cmd.env("LOOM_DIR", config.state_dir().to_string_lossy().as_ref());
    cmd.env("LOOM_RUN_DIR", run_dir.to_string_lossy().as_ref());
    if let Some(m) = model {
        cmd.env("LOOM_MODEL", m);
    }

    let status = cmd.status()?;
    if !status.success() {
        bail!("script exited with {}", status);
    }
    Ok(())
}
