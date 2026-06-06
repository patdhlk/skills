use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;

fn pds() -> Command {
    Command::cargo_bin("pds").unwrap()
}

/// Write `body` to `path` and mark it executable (unix). Used to drop fake
/// builder scripts into a tempdir for E2E tests.
#[cfg(unix)]
fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
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

// ---------------------------------------------------------------------------
// `pds build` E2E with fake builders (unix-only: scripts need the exec bit)
// ---------------------------------------------------------------------------

/// A fake sphinx-build that, invoked as `... -b needs <srcdir> <outdir>`,
/// writes a needs.json into the final argument (outdir) and exits 0.
#[cfg(unix)]
const FAKE_SPHINX_OK: &str = r#"#!/bin/sh
echo "fake-sphinx: building" >&2
echo "chatty line on stdout"
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
printf '{"versions":{}}' > "$outdir/needs.json"
exit 0
"#;

/// A fake sphinx-build that prints to both streams and exits non-zero
/// without producing any needs.json.
#[cfg(unix)]
const FAKE_SPHINX_FAIL: &str = r#"#!/bin/sh
echo "fake-sphinx: corpus is broken" >&2
echo "noise on stdout"
exit 1
"#;

/// A fake sphinx-build that exits 0 but writes nothing (adapter mis-ran case).
#[cfg(unix)]
const FAKE_SPHINX_NOOP: &str = r#"#!/bin/sh
echo "fake-sphinx: did nothing" >&2
exit 0
"#;

/// Set up a tempdir project whose ubproject.toml routes `sphinx-build` to a
/// fake script `script_name` with the given `body`. needs_json defaults to
/// `<spec_dir>/_build/needs/needs.json`. Returns (tempdir, config_path).
#[cfg(unix)]
fn sphinx_project(script_name: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join(script_name);
    write_script(&script, body);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();
    (tmp, config)
}

#[cfg(unix)]
#[test]
fn build_sphinx_success_emits_clean_outcome_with_needs_json_path() {
    let (tmp, config) = sphinx_project("fake-sphinx.sh", FAKE_SPHINX_OK);
    let expected = tmp.path().join("spec/_build/needs/needs.json");

    let assert = pds().arg("build").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "build");
    let needs = json["needs_json"].as_str().expect("needs_json string");
    assert!(
        needs.ends_with("spec/_build/needs/needs.json"),
        "got: {needs}"
    );
    assert!(
        Path::new(needs).exists(),
        "the builder should have produced needs.json at {needs}"
    );
    assert!(expected.exists(), "expected default path to exist");
}

#[cfg(unix)]
#[test]
fn build_failure_emits_failed_outcome_with_findings() {
    let (_tmp, config) = sphinx_project("fake-sphinx.sh", FAKE_SPHINX_FAIL);

    let assert = pds().arg("build").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "build");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f["check"], "build");
    assert_eq!(f["severity"], "error");
    assert!(f["need"].is_null());
    assert!(
        f["message"].as_str().unwrap().contains("status 1"),
        "message should name the exit status, got: {}",
        f["message"]
    );
}

#[cfg(unix)]
#[test]
fn build_exit_zero_but_no_needs_json_is_tool_error() {
    let (_tmp, config) = sphinx_project("fake-sphinx.sh", FAKE_SPHINX_NOOP);

    let assert = pds().arg("build").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "build");
    assert_eq!(json["error"]["kind"], "tool");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("needs.json"),
        "tool error should name the expected path, got: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn build_missing_builder_program_is_tool_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    // Point sphinx_command at a script that does not exist / is not executable.
    let missing = root.join("does-not-exist.sh");
    let toml = format!(
        "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n",
        missing.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("build").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "build");
    assert_eq!(json["error"]["kind"], "tool");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("does-not-exist.sh"),
        "tool error should name the program, got: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn build_child_chatter_stays_off_pds_stdout() {
    // FAKE_SPHINX_OK writes to both stdout and stderr. pds must keep its own
    // stdout to exactly one JSON object; the child's stdout lands on stderr.
    let (_tmp, config) = sphinx_project("fake-sphinx.sh", FAKE_SPHINX_OK);

    let assert = pds().arg("build").arg("--config").arg(&config).assert();
    let out = assert.success().get_output().clone();

    // stdout parses as exactly one JSON object — no extra "chatty line".
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    let _: Value = serde_json::from_str(stdout.trim()).expect("stdout is exactly one JSON object");
    assert!(
        !stdout.contains("chatty line on stdout"),
        "child stdout must not leak onto pds stdout, got: {stdout}"
    );
    // The child's chatter appears on pds stderr.
    let stderr = String::from_utf8(out.stderr.clone()).unwrap();
    assert!(
        stderr.contains("chatty line on stdout") && stderr.contains("fake-sphinx: building"),
        "child output should be on pds stderr, got: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn build_ubc_via_path_succeeds() {
    // Inject a fake `ubc` via PATH. It is invoked as `ubc build needs --outpath <path>`.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake_ubc = r#"#!/bin/sh
# args: build needs --outpath <path>
echo "fake-ubc running" >&2
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--outpath" ]; then out="$2"; shift 2; else shift; fi
done
mkdir -p "$(dirname "$out")"
printf '{"versions":{}}' > "$out"
exit 0
"#;
    write_script(&bin.join("ubc"), fake_ubc);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    std::fs::write(
        &config,
        "[tool.patdhlk-skills]\nbuilder = \"ubc\"\nspec_dir = \"spec\"\n",
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let assert = pds()
        .arg("build")
        .arg("--config")
        .arg(&config)
        .env("PATH", path)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "build");
    let needs = json["needs_json"].as_str().unwrap();
    assert!(
        needs.ends_with("spec/_build/needs/needs.json"),
        "got: {needs}"
    );
    assert!(Path::new(needs).exists());
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
