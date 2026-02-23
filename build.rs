use std::fs;
use std::process::Command;

fn main() {
    // Auto-increment build number
    let build_file = format!("{}/.loom_build_number", env!("HOME"));
    let build_num: u64 = fs::read_to_string(&build_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
        + 1;
    fs::write(&build_file, build_num.to_string()).ok();

    // Git short hash
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=LOOM_BUILD_NUM={}", build_num);
    println!("cargo:rustc-env=LOOM_GIT_HASH={}", git_hash);
}
