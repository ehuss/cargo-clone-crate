use anyhow::Error;
use cargo_clone::{CloneMethodKind, Cloner};
use tempfile::TempDir;

fn clone(
    method_name: &str,
    spec: &str,
    version: Option<&str>,
    extra: &[&str],
) -> Result<TempDir, Error> {
    let td = tempfile::tempdir()?;
    eprintln!("temp directory: {:?}", td.path());
    let mut cloner = Cloner::new();
    cloner.set_out_dir(td.path());
    cloner.clone(
        CloneMethodKind::from(method_name).unwrap(),
        spec,
        version,
        extra,
    )?;
    Ok(td)
}

fn assert_err(err: Result<TempDir, Error>, expected: &str) {
    let err = err.expect_err("got Ok");
    let s = err.to_string();
    if !s.contains(expected) {
        panic!("Expected: {}\nGot: {}\n", expected, s);
    }
}

fn assert_downloaded(dir: &TempDir, path: &str) {
    let path = dir.path().join(path);
    if !path.exists() {
        panic!("Expected download to {:?}", path);
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
    let td = clone("crate", "bitflags", Some("=1.0.5"), &[]).unwrap();
    assert_downloaded(&td, "bitflags-1.0.5");
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
// Fossil tests cause a lot of trouble on CI.
// Currently chiselapp.com is having a certificate problem
// (see https://groups.google.com/g/comp.lang.tcl/c/fgAfAFfiHPo)
#[ignore]
fn clone_fossil() {
    // Ensure `fossil` is a spawnable process on the machine
    assert!(std::process::Command::new("fossil").output().is_ok());

    let td = clone("fossil", "rs-graph", None, &["graph.fossil"]).unwrap();
    assert_downloaded(&td, "graph.fossil");
}

#[test]
fn clone_git_args() {
    let td = clone("git", "bitflags", None, &["--depth=1", "bf"]).unwrap();
    assert_downloaded(&td, "bf");
}
