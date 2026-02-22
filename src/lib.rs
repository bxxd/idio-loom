pub mod claude;
pub mod config;
pub mod loom;
pub mod meta;
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
