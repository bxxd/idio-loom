use idio_loom::prompt::resolve_at_refs;
use std::fs;
use tempfile::TempDir;

#[test]
fn single_at_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("role.md"), "You are a researcher.").unwrap();

    let result = resolve_at_refs("@role.md", tmp.path());
    assert_eq!(result, "You are a researcher.");
}

#[test]
fn inline_at_refs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("header.md"), "HEADER").unwrap();
    fs::write(tmp.path().join("footer.md"), "FOOTER").unwrap();

    let result = resolve_at_refs("Start @header.md middle @footer.md end", tmp.path());
    assert_eq!(result, "Start HEADER middle FOOTER end");
}

#[test]
fn no_refs_passthrough() {
    let tmp = TempDir::new().unwrap();
    let result = resolve_at_refs("plain text no refs", tmp.path());
    assert_eq!(result, "plain text no refs");
}

#[test]
fn missing_file_warns_keeps_ref() {
    let tmp = TempDir::new().unwrap();
    // Single @file that doesn't exist — should return the ref text
    let result = resolve_at_refs("@missing.md", tmp.path());
    assert_eq!(result, "@missing.md");
}
