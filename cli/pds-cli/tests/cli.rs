use assert_cmd::Command;
use serde_json::Value;

fn pds() -> Command {
    Command::cargo_bin("pds").unwrap()
}

#[test]
fn build_without_config_emits_config_error() {
    let tmp = tempfile::tempdir().unwrap();
    let assert = pds().arg("build").current_dir(tmp.path()).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "build");
    assert_eq!(json["error"]["kind"], "config");
    assert!(json["error"]["message"].is_string());
    assert!(!out.stderr.is_empty(), "stderr should carry a human line");
}

#[test]
fn check_without_config_emits_config_error() {
    let tmp = tempfile::tempdir().unwrap();
    let assert = pds().arg("check").current_dir(tmp.path()).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "check");
    assert_eq!(json["error"]["kind"], "config");
    assert!(!out.stderr.is_empty());
}

#[test]
fn config_flag_pointing_at_missing_file_is_config_error() {
    let assert = pds()
        .arg("build")
        .arg("--config")
        .arg("/nonexistent/ubproject.toml")
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "build");
    assert_eq!(json["error"]["kind"], "config");
}

#[test]
fn resolved_project_yields_not_implemented_tool_error() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("ubproject.toml");
    std::fs::write(&config, "").unwrap();

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "check");
    assert_eq!(json["error"]["kind"], "tool");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not implemented")
    );
}

#[test]
fn version_flag_succeeds() {
    pds()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains("0.1.0"));
}

#[test]
fn unknown_subcommand_exits_two() {
    pds().arg("frobnicate").assert().code(2);
}

/// Vertical slice: a project whose ubproject.toml has `builder = "make"` must cause
/// `pds build` to exit 2 with `error.kind == "config"` and a message naming "make".
#[test]
fn bad_builder_value_surfaces_as_config_error() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("ubproject.toml");
    std::fs::write(&config, "[tool.patdhlk-skills]\nbuilder = \"make\"\n").unwrap();

    let assert = pds().arg("build").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "build");
    assert_eq!(json["error"]["kind"], "config");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("make"),
        "error message should name the bad value \"make\", got: {msg}"
    );
}
