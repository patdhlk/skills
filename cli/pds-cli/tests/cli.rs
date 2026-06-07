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

// ---------------------------------------------------------------------------
// `pds status` / `pds next` E2E — backlog queries (unix-only: fake builders).
// ---------------------------------------------------------------------------

/// A fake sphinx-build for the query verbs: invoked as `... -b needs <src>
/// <outdir>`, it writes a needs.json into the outdir containing issues in
/// several statuses (plus a non-issue need and a no-status issue), then exits 0.
#[cfg(unix)]
const FAKE_SPHINX_BACKLOG: &str = r#"#!/bin/sh
echo "fake-sphinx: building backlog" >&2
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
cat > "$outdir/needs.json" <<'JSON'
{
  "current_version": "",
  "project": "t",
  "versions": { "": { "needs": {
    "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"first ready","status":"ready-for-agent","links":["FEAT_0001"]},
    "ISSUE_0002": {"id":"ISSUE_0002","type":"issue","title":"done one","status":"done"},
    "ISSUE_0003": {"id":"ISSUE_0003","type":"issue","title":"triage","status":"needs-triage"},
    "ISSUE_0004": {"id":"ISSUE_0004","type":"issue","title":"second ready","status":"ready-for-agent"},
    "ISSUE_0005": {"id":"ISSUE_0005","type":"issue","title":"no status"},
    "FEAT_0001":  {"id":"FEAT_0001","type":"feat","title":"a feature","status":"done"}
  } } }
}
JSON
exit 0
"#;

/// Set up a project whose fake sphinx-build writes the backlog needs.json, with
/// an `issue` role declared (and the matching `[[needs.types]]`). `extra` is
/// appended to the `[tool.patdhlk-skills]` table (e.g. an issue_backend line).
/// `roles` is the body of the `[tool.patdhlk-skills.roles]` table; when empty
/// the roles table is omitted entirely.
#[cfg(unix)]
fn backlog_project(extra: &str, roles: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, FAKE_SPHINX_BACKLOG);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let roles_table = if roles.is_empty() {
        String::new()
    } else {
        format!("\n[tool.patdhlk-skills.roles]\n{roles}")
    };
    let toml = format!(
        "[[needs.types]]\ndirective = \"issue\"\n\n\
         [[needs.types]]\ndirective = \"feat\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n{extra}\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n{roles_table}",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();
    (tmp, config)
}

#[cfg(unix)]
#[test]
fn status_emits_per_status_counts() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\nfeature = \"feat\"\n");

    let assert = pds().arg("status").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "status");
    let counts = &json["counts"];
    assert_eq!(counts["ready-for-agent"], 2);
    assert_eq!(counts["done"], 1);
    assert_eq!(counts["needs-triage"], 1);
    assert_eq!(counts["none"], 1);
    // The non-issue feat (status done) must not bleed into the issue tally.
    assert_eq!(json["total"], 5);
    // The feat's "done" must not inflate the done count.
    assert!(counts.get("feat").is_none());
}

#[cfg(unix)]
#[test]
fn next_emits_lowest_ready_issue_with_links() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds().arg("next").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "next");
    let issue = &json["issue"];
    assert_eq!(issue["id"], "ISSUE_0001");
    assert_eq!(issue["title"], "first ready");
    assert_eq!(issue["status"], "ready-for-agent");
    let links = issue["links"].as_array().expect("links array");
    assert_eq!(links, &vec![Value::String("FEAT_0001".to_string())]);
    assert!(json["reason"].is_null());
}

/// A fake sphinx-build whose needs.json has issues but none ready-for-agent.
#[cfg(unix)]
const FAKE_SPHINX_NONE_READY: &str = r#"#!/bin/sh
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
cat > "$outdir/needs.json" <<'JSON'
{
  "current_version": "",
  "project": "t",
  "versions": { "": { "needs": {
    "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"a","status":"done"},
    "ISSUE_0002": {"id":"ISSUE_0002","type":"issue","title":"b","status":"in-progress"}
  } } }
}
JSON
exit 0
"#;

#[cfg(unix)]
#[test]
fn next_with_no_ready_issue_is_clean_none_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, FAKE_SPHINX_NONE_READY);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"issue\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n\n\
         [tool.patdhlk-skills.roles]\nissue = \"issue\"\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("next").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "next");
    assert!(json["issue"].is_null());
    assert_eq!(json["reason"], "none-ready");
}

#[cfg(unix)]
#[test]
fn status_github_backend_is_tool_error_naming_gh() {
    let (_tmp, config) = backlog_project("issue_backend = \"github\"\n", "issue = \"issue\"\n");

    let assert = pds().arg("status").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "status");
    assert_eq!(json["error"]["kind"], "tool");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(msg.contains("gh issue list"), "got: {msg}");
}

#[cfg(unix)]
#[test]
fn next_github_backend_is_tool_error_naming_gh_command() {
    let (_tmp, config) = backlog_project("issue_backend = \"github\"\n", "issue = \"issue\"\n");

    let assert = pds().arg("next").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "next");
    assert_eq!(json["error"]["kind"], "tool");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("gh issue list --label ready-for-agent"),
        "got: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn status_missing_issue_role_is_config_error() {
    // roles table present but with no `issue` entry.
    let (_tmp, config) = backlog_project("", "feature = \"feat\"\n");

    let assert = pds().arg("status").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "status");
    assert_eq!(json["error"]["kind"], "config");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("issue"),
        "error must name the missing role, got: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn next_missing_issue_role_is_config_error() {
    // empty roles table (omitted entirely) => no issue role.
    let (_tmp, config) = backlog_project("", "");

    let assert = pds().arg("next").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "next");
    assert_eq!(json["error"]["kind"], "config");
}

/// A fake sphinx-build that exits non-zero without producing needs.json.
/// Reuses FAKE_SPHINX_FAIL semantics for the query verbs.
#[cfg(unix)]
#[test]
fn next_build_failure_surfaces_findings_under_next_verb() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, FAKE_SPHINX_FAIL);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"issue\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n\n\
         [tool.patdhlk-skills.roles]\nissue = \"issue\"\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("next").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    // Failed build is reported under the invoked verb's envelope.
    assert_eq!(json["verb"], "next");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "build");
}

// ---------------------------------------------------------------------------
// `pds lint` / `pds check` lint-integration E2E (unix-only: fake builders).
// ---------------------------------------------------------------------------

/// A fake sphinx-build that drops a `built.marker` file next to itself (so a
/// test can prove whether the builder was invoked) and writes a needs.json
/// with one `req` need whose body uses an unenumerated quantifier + weasel word.
#[cfg(unix)]
const FAKE_SPHINX_LINT_VIOLATION: &str = r#"#!/bin/sh
touch "$(dirname "$0")/built.marker"
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
cat > "$outdir/needs.json" <<'JSON'
{
  "current_version": "",
  "project": "t",
  "versions": { "": { "needs": {
    "REQ_0001": {"id":"REQ_0001","type":"req","title":"r","content":"All inputs shall be robust."}
  } } }
}
JSON
exit 0
"#;

/// A fake sphinx-build that drops the marker and writes a clean `req` corpus
/// (enumerated quantifier, no weasel words).
#[cfg(unix)]
const FAKE_SPHINX_LINT_CLEAN: &str = r#"#!/bin/sh
touch "$(dirname "$0")/built.marker"
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
cat > "$outdir/needs.json" <<'JSON'
{
  "current_version": "",
  "project": "t",
  "versions": { "": { "needs": {
    "REQ_0001": {"id":"REQ_0001","type":"req","title":"r","content":"All of the declared inputs shall be validated within 5 ms."}
  } } }
}
JSON
exit 0
"#;

/// Set up a project routed at `body`, declaring `req` in [[needs.types]], with
/// the given `[tool.patdhlk-skills.lint]` table body appended (may be empty to
/// omit the table). Returns (tempdir, config_path).
#[cfg(unix)]
fn lint_project(body: &str, lint_table: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, body);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"req\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n{lint_table}",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();
    (tmp, config)
}

/// A lint table flagging the weasel word "robust" and quantifier "all" on req.
#[cfg(unix)]
const LINT_TABLE_REQ: &str = "\n[tool.patdhlk-skills.lint.weasel_words]\n\
     words = [\"robust\"]\ndirectives = [\"req\"]\n\n\
     [tool.patdhlk-skills.lint.unenumerated_quantifiers]\n\
     quantifiers = [\"all\"]\ndirectives = [\"req\"]\n";

#[cfg(unix)]
#[test]
fn lint_with_no_table_is_clean_and_does_not_build() {
    // No lint table at all: clean exit 0, empty findings, and the builder must
    // NOT have run (no marker file).
    let (tmp, config) = lint_project(FAKE_SPHINX_LINT_VIOLATION, "");

    let assert = pds().arg("lint").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "lint");
    let findings = json["findings"].as_array().expect("findings array");
    assert!(findings.is_empty(), "no lint table => empty findings");
    assert!(
        !tmp.path().join("built.marker").exists(),
        "lint with no enabled rules must NOT invoke the builder"
    );
}

#[cfg(unix)]
#[test]
fn lint_with_violations_exits_one_with_namespaced_findings() {
    let (_tmp, config) = lint_project(FAKE_SPHINX_LINT_VIOLATION, LINT_TABLE_REQ);

    let assert = pds().arg("lint").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "lint");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(
        findings.len(),
        2,
        "weasel word + quantifier => two findings"
    );
    for f in findings {
        assert_eq!(f["need"], "REQ_0001", "need id must be populated");
        assert_eq!(f["severity"], "error");
        assert!(
            f["check"].as_str().unwrap().starts_with("lint:"),
            "check must carry the lint: prefix, got: {}",
            f["check"]
        );
    }
    // needs_json reported (fresh corpus was built).
    assert!(json["needs_json"].as_str().unwrap().ends_with("needs.json"));
}

#[cfg(unix)]
#[test]
fn lint_clean_corpus_exits_zero() {
    let (_tmp, config) = lint_project(FAKE_SPHINX_LINT_CLEAN, LINT_TABLE_REQ);

    let assert = pds().arg("lint").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "lint");
    assert!(json["findings"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn check_with_lint_table_appends_lint_findings_after_builder() {
    // Clean builder gate, then lint fires on the corpus => exit 1 with lint
    // findings present alongside the (empty) builder findings.
    let (_tmp, config) = lint_project(FAKE_SPHINX_LINT_VIOLATION, LINT_TABLE_REQ);

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 2, "two lint findings appended");
    assert!(
        findings
            .iter()
            .all(|f| f["check"].as_str().unwrap().starts_with("lint:")),
        "all findings are lint findings (builder gate passed)"
    );
    assert!(findings.iter().all(|f| f["need"] == "REQ_0001"));
}

#[cfg(unix)]
#[test]
fn lint_required_sections_on_undeclared_directive_is_config_error() {
    // ubproject.toml declares only "issue" in [[needs.types]] but the lint
    // table's required_sections references "arch-decision" which is not
    // declared.  Config::load must reject this at load time: pds lint exits 2,
    // stdout JSON has error.kind == "config", and the message names
    // "arch-decision".  The builder must NOT be invoked (no marker file).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(
        &script,
        r#"#!/bin/sh
touch "$(dirname "$0")/built.marker"
exit 0
"#,
    );
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"issue\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n\n\
         [tool.patdhlk-skills.lint.required_sections]\n\
         arch-decision = [\"Context\", \"Decision\", \"Consequences\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("lint").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "lint");
    assert_eq!(json["error"]["kind"], "config");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("arch-decision"),
        "error message must name the undeclared directive, got: {msg}"
    );
    assert!(
        !root.join("built.marker").exists(),
        "builder must NOT be invoked when config is invalid"
    );
    assert!(!out.stderr.is_empty(), "stderr should carry a human line");
}

/// A fake sphinx-build that writes a needs.json with TWO req needs carrying the
/// same violating body: REQ_0001 has `"status":"done"` (exempt by default) and
/// REQ_0002 has no status field (never exempt). Pins ISSUE_0018.
#[cfg(unix)]
const FAKE_SPHINX_LINT_EXEMPT_MIX: &str = r#"#!/bin/sh
touch "$(dirname "$0")/built.marker"
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
cat > "$outdir/needs.json" <<'JSON'
{
  "current_version": "",
  "project": "t",
  "versions": { "": { "needs": {
    "REQ_0001": {"id":"REQ_0001","type":"req","title":"r","content":"All inputs shall be robust.","status":"done"},
    "REQ_0002": {"id":"REQ_0002","type":"req","title":"s","content":"All inputs shall be robust."}
  } } }
}
JSON
exit 0
"#;

#[cfg(unix)]
#[test]
fn lint_exempts_done_status_needs_but_not_statusless_ones() {
    // ISSUE_0018 contract through the binary: gate.exempt_statuses (default
    // done/wontfix) excludes terminal needs from lint; absence of a status
    // is not "done".
    let (_tmp, config) = lint_project(FAKE_SPHINX_LINT_EXEMPT_MIX, LINT_TABLE_REQ);

    let assert = pds().arg("lint").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    let findings = json["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().all(|f| f["need"] == "REQ_0002"),
        "only the statusless need may be flagged, got: {findings:?}"
    );
    assert!(
        !findings.is_empty(),
        "the statusless need must be flagged (no status is never exempt)"
    );
}

#[cfg(unix)]
#[test]
fn check_with_builder_failure_does_not_run_lint() {
    // Builder fails => corpus suspect => lint must NOT run; only the build
    // finding is present.
    let (_tmp, config) = lint_project(FAKE_SPHINX_FAIL, LINT_TABLE_REQ);

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1, "only the builder finding");
    assert_eq!(findings[0]["check"], "build");
    assert!(
        findings
            .iter()
            .all(|f| f["check"].as_str().unwrap() != "lint:weasel-words"),
        "no lint findings when the build failed"
    );
}

// ---------------------------------------------------------------------------
// `pds search` / `pds dedup` E2E — retrieval verbs (unix-only: fake builders).
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn search_ranks_hits_with_engine_and_normalized_scores() {
    // No roles table: search must not require the issue role.
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("search")
        .arg("ready")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "search");
    assert_eq!(json["engine"], "bm25");
    let hits = json["hits"].as_array().expect("hits array");
    assert!(!hits.is_empty(), "\"ready\" appears in two issue titles");
    for h in hits {
        assert!(h["id"].is_string());
        assert!(h["type"].is_string());
        assert!(h["title"].is_string());
        // status may be null (ISSUE_0005 has none) but the key must exist.
        assert!(h.get("status").is_some());
        let score = h["score"].as_f64().expect("score is a number");
        assert!(score > 0.0 && score <= 1.0, "score {score} outside (0, 1]");
    }
    let ids: Vec<&str> = hits.iter().map(|h| h["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"ISSUE_0001"), "got: {ids:?}");
    assert!(ids.contains(&"ISSUE_0004"), "got: {ids:?}");
}

#[cfg(unix)]
#[test]
fn search_with_no_matches_is_clean_empty_hits() {
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("search")
        .arg("zebra astronomy quantum")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "search");
    assert!(json["hits"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn search_empty_query_is_config_error() {
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("search")
        .arg("   ")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "search");
    assert_eq!(json["error"]["kind"], "config");
}

#[cfg(unix)]
#[test]
fn search_github_backend_is_tool_error_naming_gh() {
    let (_tmp, config) = backlog_project("issue_backend = \"github\"\n", "");

    let assert = pds()
        .arg("search")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "search");
    assert_eq!(json["error"]["kind"], "tool");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("gh search issues"),
        "got: {}",
        json["error"]["message"]
    );
}

#[cfg(unix)]
#[test]
fn search_build_failure_surfaces_findings_under_search_verb() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, FAKE_SPHINX_FAIL);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds()
        .arg("search")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "search");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "build");
}

#[cfg(unix)]
#[test]
fn dedup_near_copy_of_existing_issue_exits_one_duplicate() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    // "first ready" is ISSUE_0001's exact title — a near-copy candidate.
    let assert = pds()
        .arg("dedup")
        .arg("first ready")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["engine"], "bm25");
    assert_eq!(json["verdict"], "duplicate");
    assert!(json["threshold"].as_f64().is_some());
    let hits = json["hits"].as_array().expect("hits array");
    assert_eq!(hits[0]["id"], "ISSUE_0001");
    assert!(hits[0]["score"].as_f64().unwrap() >= json["threshold"].as_f64().unwrap());
}

#[cfg(unix)]
#[test]
fn dedup_novel_candidate_exits_zero_unique() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("zebra astronomy quantum")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["verdict"], "unique");
    assert!(json["hits"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn dedup_non_issue_hit_does_not_gate() {
    // "a feature" is FEAT_0001's exact title: a strong feat-typed hit must
    // NOT flip the verdict (ADR_0021) — exit 0, hits still listed.
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\nfeature = \"feat\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("a feature")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verdict"], "unique");
    let hits = json["hits"].as_array().unwrap();
    assert!(
        hits.iter().any(|h| h["id"] == "FEAT_0001"),
        "the feat hit must still be listed, got: {hits:?}"
    );
}

#[cfg(unix)]
#[test]
fn dedup_threshold_flag_flips_the_verdict() {
    // The candidate "ready zzzqqq" half-matches ISSUE_0001/ISSUE_0004
    // ("ready" matches, "zzzqqq" is unknown): its normalized score lands at
    // ~0.43 — deterministic, well away from the clamp ceiling. The same
    // candidate must be unique at --threshold 0.5 and duplicate at
    // --threshold 0.25, proving the flag governs the gate.
    //
    // NOTE: a single-token query like "ready" is the wrong probe here — the
    // ×3 title weighting saturates tf and clamps the score to 1.0.
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("ready zzzqqq")
        .arg("--threshold")
        .arg("0.5")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verdict"], "unique");
    assert_eq!(json["threshold"], 0.5);
    assert!(!json["hits"].as_array().unwrap().is_empty());

    let assert = pds()
        .arg("dedup")
        .arg("ready zzzqqq")
        .arg("--threshold")
        .arg("0.25")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verdict"], "duplicate");
    assert_eq!(json["threshold"], 0.25);
}

#[cfg(unix)]
#[test]
fn dedup_invalid_threshold_flag_is_config_error() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("anything")
        .arg("--threshold")
        .arg("1.5")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "config");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--threshold"),
        "got: {}",
        json["error"]["message"]
    );
}

#[cfg(unix)]
#[test]
fn dedup_empty_candidate_is_config_error() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("  ")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "config");
}

#[cfg(unix)]
#[test]
fn dedup_missing_issue_role_is_config_error() {
    // dedup gates on issue-typed hits, so it needs the issue role.
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("dedup")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "config");
}

#[cfg(unix)]
#[test]
fn dedup_github_backend_is_tool_error() {
    let (_tmp, config) = backlog_project("issue_backend = \"github\"\n", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "tool");
}

// ---------------------------------------------------------------------------
// `pds verdict-check` E2E — verdict gate (unix-only: fake builders).
// ---------------------------------------------------------------------------

/// Write a fake sphinx-build whose needs.json contains one ready-for-agent
/// issue plus (optionally) a verdict need with the given fields JSON.
/// Returns (tempdir, config_path). The config declares issue+verdict types,
/// the verdict role, a triage rubric, and the require/statuses tables.
#[cfg(unix)]
fn verdict_project(verdict_fields: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    let verdict = match verdict_fields {
        Some(fields) => format!(
            r#","VERDICT_ISSUE_0001": {{"id":"VERDICT_ISSUE_0001","type":"verdict","title":"v",{fields}}}"#
        ),
        None => String::new(),
    };
    let needs = format!(
        r#"{{"current_version":"","project":"t","versions":{{"":{{"needs":{{
            "ISSUE_0001": {{"id":"ISSUE_0001","type":"issue","title":"the title","status":"ready-for-agent","content":"the body"}}{verdict}
        }}}}}}}}"#
    );
    let script_body = format!(
        "#!/bin/sh\noutdir=\"$(eval echo \\${{$#}})\"\nmkdir -p \"$outdir\"\ncat > \"$outdir/needs.json\" <<'JSON'\n{needs}\nJSON\nexit 0\n"
    );

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, &script_body);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"issue\"\n\n\
         [[needs.types]]\ndirective = \"verdict\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n\n\
         [tool.patdhlk-skills.roles]\nissue = \"issue\"\nverdict = \"verdict\"\n\n\
         [tool.patdhlk-skills.rubrics.triage]\naxes = [\"category\", \"state\"]\n\n\
         [tool.patdhlk-skills.verdicts]\nrequire = {{ issue = \"triage\" }}\n\
         statuses = [\"ready-for-agent\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();
    (tmp, config)
}

#[cfg(unix)]
#[test]
fn verdict_check_missing_exits_one() {
    let (_tmp, config) = verdict_project(None);

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "verdict-check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:missing");
    assert_eq!(findings[0]["need"], "ISSUE_0001");
    assert!(json["needs_json"].as_str().unwrap().ends_with("needs.json"));
}

#[cfg(unix)]
#[test]
fn verdict_check_failing_axes_exits_one() {
    let fp = pds_core::fingerprint("the title", "the body");
    let fields = format!(r#""rubric":"triage","axes_failed":"state","fingerprint":"{fp}""#);
    let (_tmp, config) = verdict_project(Some(&fields));

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:failing");
    assert_eq!(findings[0]["need"], "ISSUE_0001");
    assert!(findings[0]["message"].as_str().unwrap().contains("state"));
}

#[cfg(unix)]
#[test]
fn verdict_check_passing_fresh_verdict_exits_zero() {
    let fp = pds_core::fingerprint("the title", "the body");
    let fields = format!(r#""rubric":"triage","axes_failed":"","fingerprint":"{fp}""#);
    let (_tmp, config) = verdict_project(Some(&fields));

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "verdict-check");
    assert!(json["findings"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn verdict_check_stale_message_carries_recomputed_fingerprint() {
    let fields = r#""rubric":"triage","axes_failed":"","fingerprint":"sha256:0000000000000000""#;
    let (_tmp, config) = verdict_project(Some(fields));

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:stale");
    let expected = pds_core::fingerprint("the title", "the body");
    assert!(
        findings[0]["message"].as_str().unwrap().contains(&expected),
        "stale message must carry the recomputed fingerprint"
    );
}

#[cfg(unix)]
#[test]
fn verdict_check_malformed_names_the_verdict() {
    let fp = pds_core::fingerprint("the title", "the body");
    let fields = format!(r#""rubric":"triage","axes_failed":"vibes","fingerprint":"{fp}""#);
    let (_tmp, config) = verdict_project(Some(&fields));

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:malformed");
    assert_eq!(findings[0]["need"], "VERDICT_ISSUE_0001");
}

#[cfg(unix)]
#[test]
fn verdict_check_without_table_is_clean_and_does_not_build() {
    // No verdicts table: clean exit 0, and the builder must NOT run.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(
        &script,
        "#!/bin/sh\ntouch \"$(dirname \"$0\")/built.marker\"\nexit 0\n",
    );
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "verdict-check");
    assert!(json["findings"].as_array().unwrap().is_empty());
    assert!(
        !root.join("built.marker").exists(),
        "no verdicts table must mean no build"
    );
}

#[cfg(unix)]
#[test]
fn verdict_check_missing_verdict_role_is_config_error() {
    // verdicts table present but the role map has no `verdict` entry.
    let (_tmp, config) = verdict_project(None);
    let toml = std::fs::read_to_string(&config).unwrap();
    let toml = toml.replace("verdict = \"verdict\"\n", "");
    std::fs::write(&config, toml).unwrap();

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "verdict-check");
    assert_eq!(json["error"]["kind"], "config");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("verdict")
    );
}

#[cfg(unix)]
#[test]
fn verdict_check_undeclared_rubric_in_require_is_config_error() {
    let (_tmp, config) = verdict_project(None);
    let toml = std::fs::read_to_string(&config).unwrap();
    let toml = toml.replace(
        "require = { issue = \"triage\" }",
        "require = { issue = \"missing-rubric\" }",
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds()
        .arg("verdict-check")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["kind"], "config");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing-rubric")
    );
}

// ---------------------------------------------------------------------------
// `pds check` verdict-integration E2E — Task 5 (unix-only: fake builders).
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn check_with_verdicts_table_appends_verdict_findings() {
    // Clean builder, no lint table, missing verdict ⇒ pds check exits 1 with
    // the verdict finding in the one findings array.
    let (_tmp, config) = verdict_project(None);

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:missing");
    assert!(json["needs_json"].as_str().unwrap().ends_with("needs.json"));
}

#[cfg(unix)]
#[test]
fn check_clean_when_verdict_passes_and_fresh() {
    let fp = pds_core::fingerprint("the title", "the body");
    let fields = format!(r#""rubric":"triage","axes_failed":"","fingerprint":"{fp}""#);
    let (_tmp, config) = verdict_project(Some(&fields));

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json["findings"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn check_builder_failure_skips_verdict_check() {
    // Failing builder ⇒ only the build finding; verdict-check must not run.
    let (_tmp, config) = verdict_project(None);
    // Point the config's sphinx_command at a failing script.
    let toml = std::fs::read_to_string(&config).unwrap();
    let root = std::path::Path::new(&config)
        .parent()
        .unwrap()
        .to_path_buf();
    let fail = root.join("fail-sphinx.sh");
    write_script(&fail, FAKE_SPHINX_FAIL);
    let toml = {
        // Replace the sphinx_command line wholesale.
        let mut out = String::new();
        for line in toml.lines() {
            if line.starts_with("sphinx_command = ") {
                out.push_str(&format!("sphinx_command = [\"{}\"]\n", fail.display()));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    };
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "build");
}

/// A fake sphinx-build for the lint+verdict coexistence test: writes a
/// needs.json with one `req` need (no status, body uses the weasel word
/// "robust") and one `issue` need (status ready-for-agent). No verdict need is
/// present, so the verdict stage fires a `verdict:missing` finding.
#[cfg(unix)]
const FAKE_SPHINX_LINT_AND_VERDICT: &str = r#"#!/bin/sh
outdir="$(eval echo \${$#})"
mkdir -p "$outdir"
cat > "$outdir/needs.json" <<'JSON'
{
  "current_version": "",
  "project": "t",
  "versions": { "": { "needs": {
    "REQ_0001":   {"id":"REQ_0001",  "type":"req",   "title":"r",         "content":"The system shall be robust."},
    "ISSUE_0001": {"id":"ISSUE_0001","type":"issue",  "title":"the title", "content":"the body","status":"ready-for-agent"}
  } } }
}
JSON
exit 0
"#;

#[cfg(unix)]
#[test]
fn check_with_both_lint_and_verdict_tables_both_findings_coexist() {
    // Both lint and verdict stages must run and contribute findings.
    // Corpus: REQ_0001 (req, no status) body has weasel word "robust" →
    // lint:weasel-words fires.  ISSUE_0001 (issue, ready-for-agent) has no
    // matching verdict need → verdict:missing fires.  Total: 2 findings.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, FAKE_SPHINX_LINT_AND_VERDICT);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"req\"\n\n\
         [[needs.types]]\ndirective = \"issue\"\n\n\
         [[needs.types]]\ndirective = \"verdict\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n\n\
         [tool.patdhlk-skills.roles]\nissue = \"issue\"\nverdict = \"verdict\"\n\n\
         [tool.patdhlk-skills.lint.weasel_words]\nwords = [\"robust\"]\ndirectives = [\"req\"]\n\n\
         [tool.patdhlk-skills.rubrics.triage]\naxes = [\"category\"]\n\n\
         [tool.patdhlk-skills.verdicts]\nrequire = {{ issue = \"triage\" }}\n\
         statuses = [\"ready-for-agent\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(
        findings.len(),
        2,
        "lint finding + verdict finding must coexist"
    );
    let checks: Vec<&str> = findings
        .iter()
        .map(|f| f["check"].as_str().unwrap())
        .collect();
    assert!(
        checks.iter().any(|c| c.starts_with("lint:")),
        "one finding must carry the lint: prefix, got: {checks:?}"
    );
    assert!(
        checks.contains(&"verdict:missing"),
        "one finding must be verdict:missing, got: {checks:?}"
    );
    assert!(
        json["needs_json"].as_str().unwrap().ends_with("needs.json"),
        "needs_json must be reported on non-builder failures"
    );
}

#[cfg(unix)]
#[test]
fn dedup_build_failure_surfaces_findings_under_dedup_verb() {
    // On a failed build the exit-1 payload is findings-shaped — no verdict,
    // threshold, or engine keys. Pins the BuildFailed pass-through for dedup.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, FAKE_SPHINX_FAIL);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"issue\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n\n\
         [tool.patdhlk-skills.roles]\nissue = \"issue\"\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds()
        .arg("dedup")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "dedup");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "build");
    assert!(json.get("verdict").is_none(), "no verdict on build failure");
}
