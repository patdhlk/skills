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

// ---------------------------------------------------------------------------
// Hermetic fake-`ubc` helper. The real `ubc` exists on this machine's PATH, so
// every ubc E2E test MUST go through this helper: it writes a fake `ubc` into a
// tempdir `bin/` and returns a PATH value with that `bin/` prepended, so the
// fake always shadows the real binary.
// ---------------------------------------------------------------------------

/// Set up a tempdir project whose ubproject.toml selects `builder = "ubc"`, with
/// a fake `ubc` script `body` dropped into `<root>/bin/ubc`. Returns
/// `(tempdir, config_path, path_env)` where `path_env` is the PATH-prefixed
/// value to pass via `.env("PATH", ...)` so the fake shadows the real `ubc`.
#[cfg(unix)]
fn ubc_project(body: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    write_script(&bin.join("ubc"), body);
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
    (tmp, config, path)
}

/// A fake `ubc` whose `build needs --outpath <path>` writes a needs.json and
/// exits 0, and whose `check <spec_dir>` subcommand exits 0. Chatters on both
/// streams. The `check` branch asserts that a positional spec_dir argument is
/// present (i.e. `$2` is non-empty); if it is missing the fake exits 2 to
/// surface the bare-`ubc-check` bug.
#[cfg(unix)]
const FAKE_UBC_OK: &str = r#"#!/bin/sh
echo "fake-ubc running: $1" >&2
echo "ubc chatty stdout"
if [ "$1" = "check" ]; then
  if [ -z "$2" ]; then
    echo "fake-ubc: check requires a spec_dir argument" >&2
    exit 2
  fi
  exit 0
fi
# build needs --outpath <path>
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--outpath" ]; then out="$2"; shift 2; else shift; fi
done
mkdir -p "$(dirname "$out")"
printf '{"versions":{}}' > "$out"
exit 0
"#;

#[cfg(unix)]
#[test]
fn build_ubc_via_path_succeeds() {
    // Inject a fake `ubc` via PATH. It is invoked as `ubc build needs --outpath <path>`.
    let (_tmp, config, path) = ubc_project(FAKE_UBC_OK);

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

// ---------------------------------------------------------------------------
// `pds check` E2E — the strict gate (unix-only: scripts need the exec bit).
// ---------------------------------------------------------------------------

/// A fake sphinx-build for `pds check`: invoked as `... -W -b needs <src> <out>`,
/// it writes needs.json into the final argument (outdir) and exits 0. The `-W`
/// flag is present in argv but the fake does not need to act on it.
#[cfg(unix)]
const FAKE_SPHINX_CHECK_OK: &str = r#"#!/bin/sh
echo "fake-sphinx: checking" >&2
echo "noise on stdout"
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
printf '{"versions":{}}' > "$outdir/needs.json"
exit 0
"#;

#[cfg(unix)]
#[test]
fn check_sphinx_success_emits_clean_outcome() {
    let (tmp, config) = sphinx_project("fake-sphinx.sh", FAKE_SPHINX_CHECK_OK);
    let expected = tmp.path().join("spec/_build/needs/needs.json");

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "check");
    // findings present and empty.
    let findings = json["findings"].as_array().expect("findings array");
    assert!(findings.is_empty(), "clean check has no findings");
    // needs_json key present and the file exists.
    let needs = json["needs_json"].as_str().expect("needs_json string");
    assert!(
        needs.ends_with("spec/_build/needs/needs.json"),
        "got: {needs}"
    );
    assert!(Path::new(needs).exists());
    assert!(expected.exists(), "expected default path to exist");
}

#[cfg(unix)]
#[test]
fn check_sphinx_failure_emits_single_build_finding() {
    // FAKE_SPHINX_FAIL exits 1 without producing needs.json.
    let (_tmp, config) = sphinx_project("fake-sphinx.sh", FAKE_SPHINX_FAIL);

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "one failed step => one finding");
    let f = &findings[0];
    assert_eq!(f["check"], "build");
    assert_eq!(f["severity"], "error");
    assert!(f["need"].is_null());
    assert!(
        f["message"].as_str().unwrap().contains("status 1"),
        "message should name the exit status, got: {}",
        f["message"]
    );
    // No needs.json was produced, so the key is omitted.
    assert!(
        json.get("needs_json").is_none(),
        "needs_json must be omitted when the file was not produced"
    );
}

#[cfg(unix)]
#[test]
fn check_ubc_both_steps_pass_emits_clean_outcome() {
    let (_tmp, config, path) = ubc_project(FAKE_UBC_OK);

    let assert = pds()
        .arg("check")
        .arg("--config")
        .arg(&config)
        .env("PATH", path)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().expect("findings array");
    assert!(findings.is_empty());
    let needs = json["needs_json"].as_str().expect("needs_json string");
    assert!(Path::new(needs).exists());
}

/// A fake `ubc` whose `check` exits non-zero but whose `build needs` still
/// succeeds and writes needs.json. Pins the "even if check fails, still attempt
/// the build" semantics.
#[cfg(unix)]
const FAKE_UBC_CHECK_FAILS_BUILD_OK: &str = r#"#!/bin/sh
echo "fake-ubc: $1" >&2
if [ "$1" = "check" ]; then
  echo "fake-ubc: corpus violations" >&2
  exit 1
fi
# build needs --outpath <path>
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--outpath" ]; then out="$2"; shift 2; else shift; fi
done
mkdir -p "$(dirname "$out")"
printf '{"versions":{}}' > "$out"
exit 0
"#;

#[cfg(unix)]
#[test]
fn check_ubc_check_fails_but_build_runs_and_produces_needs_json() {
    let (_tmp, config, path) = ubc_project(FAKE_UBC_CHECK_FAILS_BUILD_OK);

    let assert = pds()
        .arg("check")
        .arg("--config")
        .arg(&config)
        .env("PATH", path)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(
        findings.len(),
        1,
        "only ubc-check failed; build still ran and passed"
    );
    assert_eq!(findings[0]["check"], "ubc-check");
    // step 2 ran despite step 1's failure: needs.json exists and is reported.
    let needs = json["needs_json"].as_str().expect("needs_json string");
    assert!(
        Path::new(needs).exists(),
        "build step must run even after check fails"
    );
}

/// A fake `ubc` that fails both `check` and `build needs` (no needs.json).
#[cfg(unix)]
const FAKE_UBC_BOTH_FAIL: &str = r#"#!/bin/sh
echo "fake-ubc: $1 failing" >&2
exit 1
"#;

#[cfg(unix)]
#[test]
fn check_ubc_both_steps_fail_emits_two_ordered_findings() {
    let (_tmp, config, path) = ubc_project(FAKE_UBC_BOTH_FAIL);

    let assert = pds()
        .arg("check")
        .arg("--config")
        .arg(&config)
        .env("PATH", path)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 2, "both steps failed => two findings");
    // Order: ubc-check first, then build.
    assert_eq!(findings[0]["check"], "ubc-check");
    assert_eq!(findings[1]["check"], "build");
    // No needs.json was produced, so the key is omitted.
    assert!(
        json.get("needs_json").is_none(),
        "needs_json must be omitted when no file was produced"
    );
}

#[cfg(unix)]
#[test]
fn check_missing_builder_program_is_tool_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let missing = root.join("does-not-exist.sh");
    let toml = format!(
        "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n",
        missing.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "check");
    assert_eq!(json["error"]["kind"], "tool");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("does-not-exist.sh"),
        "tool error should name the program, got: {msg}"
    );
}
