use anyhow::Error;
use cargo_clone::clone;
use std::fs;
use std::path::Path;

fn assert_err(err: Result<(), Error>, expected: &str) {
    let err = err.expect_err("got Ok");
    let s = err.to_string();
    if !s.contains(expected) {
        panic!("Expected: {}\nGot: {}\n", expected, s);
    }
}

fn assert_downloaded(path: &str) {
    let path = Path::new(path);
    if !path.exists() {
        panic!("Expected download to {:?}", path);
    }
    if path.is_dir() {
        fs::remove_dir_all(path).unwrap();
    } else {
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn err_both_new_old_style_version() {
    assert_err(
        clone("crate", "foo:1.2.3", Some("1.2.3"), &[]),
        "Cannot specify both",
    );
}

#[test]
fn parse_version_err() {
    assert_err(
        clone("crate", "foo", Some("abc"), &[]),
        "not a valid semver",
    );
}

#[test]
fn parse_version_empty() {
    assert_err(clone("crate", "foo", Some(""), &[]), "version is empty");
}

#[test]
fn parse_version_req_ok() {
    clone("crate", "bitflags", Some("=1.0.5"), &[]).unwrap();
    assert_downloaded("bitflags-1.0.5");
}

#[test]
fn extra_args_crate() {
    assert_err(clone("crate", "foo", None, &["extra"]), "extra arguments");
}

#[test]
fn version_with_method() {
    assert_err(
        clone("git", "bitflags", Some("1.2.3"), &[]),
        "only works with",
    );
}

#[test]
fn unknown_crate() {
    assert_err(clone("auto", "test", None, &[]), "not found");
}

#[test]
fn clone_fossil() {
    clone("fossil", "rs-graph", None, &["graph.fossil"]).unwrap();
    assert_downloaded("graph.fossil");
}

#[test]
fn clone_git_args() {
    clone("git", "bitflags", None, &["--depth=1", "bf"]).unwrap();
    assert_downloaded("bf");
}
