use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn bin() -> assert_cmd::Command {
    assert_cmd::Command::cargo_bin("arw-cli").expect("binary")
}

#[test]
fn init_and_validate_json_manifest() {
    let tmp = tempdir().expect("tmpdir");
    let out: PathBuf = tmp.path().join("my.adapter.json");

    // Init JSON
    let mut cmd = bin();
    cmd.arg("adapters")
        .arg("init")
        .arg("--out")
        .arg(&out)
        .arg("--id")
        .arg("test.sample.adapter");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Created scaffold"));

    assert!(out.exists(), "scaffolded manifest should exist");
    let text = fs::read_to_string(&out).expect("read manifest");
    assert!(
        text.contains("$schema"),
        "json output should include $schema"
    );

    // Validate
    let mut val = bin();
    val.arg("adapters")
        .arg("validate")
        .arg("--manifest")
        .arg(&out);
    val.assert()
        .success()
        .stdout(predicate::str::contains("Errors: none"));
}

#[test]
fn init_and_validate_toml_manifest() {
    let tmp = tempdir().expect("tmpdir");
    let out: PathBuf = tmp.path().join("my.adapter.toml");

    // Init TOML
    let mut cmd = bin();
    cmd.arg("adapters")
        .arg("init")
        .arg("--out")
        .arg(&out)
        .arg("--id")
        .arg("test.sample.adapter")
        .arg("--format")
        .arg("toml");
    cmd.assert().success();

    assert!(out.exists(), "scaffolded TOML manifest should exist");
    let text = fs::read_to_string(&out).expect("read manifest");
    // TOML output does not include $schema
    assert!(text.contains("id = \"test.sample.adapter\""));

    // Validate
    let mut val = bin();
    val.arg("adapters")
        .arg("validate")
        .arg("--manifest")
        .arg(&out);
    val.assert().success();
}
