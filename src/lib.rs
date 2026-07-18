pub mod backend;
pub mod config;
pub mod exec;
pub mod loom;
pub mod meta;
pub mod pattern;
pub mod prompt;
pub mod snapshot;
pub mod thread;

/// Full version string: crate version + build number + git hash
pub fn version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        ".",
        env!("LOOM_BUILD_NUM"),
        " (",
        env!("LOOM_GIT_HASH"),
        ")"
    )
}
