use idio_loom::exec::{run_with_timeout, ActivityProbe};
use std::process::Command;

/// A child whose output exceeds the OS pipe buffer (~64KB) must not deadlock.
/// Before stdout/stderr were drained concurrently, `cat` would block mid-write
/// while loom blocked writing stdin, hanging forever.
#[test]
fn large_output_does_not_deadlock() {
    let message = "x".repeat(1024 * 1024); // 1MB, well past pipe capacity
    let out = run_with_timeout(Command::new("cat"), &message, 30, ActivityProbe::default())
        .expect("cat should complete");
    assert!(out.success);
    assert_eq!(out.stdout.len(), message.len());
}

#[test]
fn large_stderr_does_not_deadlock() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("cat >&2");
    let message = "y".repeat(1024 * 1024);
    let out =
        run_with_timeout(cmd, &message, 30, ActivityProbe::default()).expect("sh should complete");
    assert!(out.success);
    assert_eq!(out.stderr.len(), message.len());
}

#[test]
fn timeout_kills_hung_child() {
    let mut cmd = Command::new("sleep");
    cmd.arg("60");
    let Err(err) = run_with_timeout(cmd, "", 1, ActivityProbe::default()) else {
        panic!("sleep 60 should be killed by the 1s timeout");
    };
    assert!(err.to_string().contains("timed out"), "got: {}", err);
}
