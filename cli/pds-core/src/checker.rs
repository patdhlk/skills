//! Checker adapter for the `pds check` verb — the strict gate.
//!
//! `pds check` has two obligations: produce a *fresh* `needs.json` AND run
//! strict, fail-closed diagnostics. How those obligations map to child
//! processes depends on the builder:
//!
//! - [`Builder::Ubc`] → **two** steps in order: (1) `ubc check`, then (2)
//!   `ubc build needs --outpath <needs_json>`. If `ubc check` fails, step 2 is
//!   *still* attempted — a stale `needs.json` is worse than none — and both
//!   failures are reported. Each failed step contributes exactly one finding.
//! - [`Builder::SphinxBuild`] → **one** step: `<sphinx_command...> -W -b needs
//!   <spec_dir> <outdir>`. The `-W` is the gate; that single build satisfies
//!   both obligations at once.
//!
//! No log text or `needs.json` content is ever parsed into findings: a step's
//! exit status is the only signal. Each failed step yields one finding named
//! after the step (`"ubc-check"` or `"build"`).
//!
//! Command construction is split out as a pure function ([`check_commands`])
//! so it can be tested without spawning a process.

use std::path::Path;
use std::process::Command;

use serde_json::{Map, Value};

use crate::builder::{
    BuildCommand, needs_json_outdir, sphinx_needs_command, step_finding, ubc_build_command,
};
use crate::config::{Builder, Config};
use crate::error::Error;
use crate::outcome::Outcome;

/// One step of `pds check`: a named child invocation. The `name` becomes the
/// `check` field of any finding the step produces (`"ubc-check"` / `"build"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckStep {
    /// Finding/step name, e.g. `"ubc-check"` or `"build"`.
    pub name: String,
    /// The fully-resolved invocation for this step.
    pub command: BuildCommand,
}

/// Construct the ordered list of check steps for `config`, run from
/// `project_root`.
///
/// - [`Builder::Ubc`] → `[ubc check, ubc build needs --outpath <needs_json>]`.
/// - [`Builder::SphinxBuild`] → `[<sphinx...> -W -b needs <spec_dir> <outdir>]`.
pub fn check_commands(config: &Config, project_root: &Path) -> Vec<CheckStep> {
    match config.builder {
        Builder::Ubc => vec![
            CheckStep {
                name: "ubc-check".to_string(),
                command: BuildCommand {
                    program: "ubc".to_string(),
                    args: vec!["check".to_string()],
                    cwd: project_root.to_path_buf(),
                },
            },
            CheckStep {
                name: "build".to_string(),
                command: ubc_build_command(config, project_root),
            },
        ],
        Builder::SphinxBuild => vec![CheckStep {
            name: "build".to_string(),
            command: sphinx_needs_command(config, project_root, true),
        }],
    }
}

/// Run the configured strict gate and classify the result per the `pds check`
/// contract.
///
/// All steps run in order with child stdout+stderr routed to pds's stderr
/// (stdout piped → stderr; stderr inherited), keeping pds's stdout reserved for
/// the single JSON envelope. Steps always run to completion even after an
/// earlier step fails (the ubc case pins this: a failed `ubc check` must not
/// skip the `needs.json` rebuild).
///
/// Outcomes:
/// - every step exits 0 **and** `needs_json` exists → [`Outcome::clean`] with
///   `{"findings": [], "needs_json": "<abs path>"}`.
/// - any step exits non-zero → [`Outcome::failed`] with one finding per failed
///   step (in order), plus `"needs_json"` when the file was produced.
/// - all steps exit 0 but `needs_json` is missing → [`Error::Tool`].
/// - a step's program cannot be spawned → [`Error::Tool`] naming the program.
pub fn run_check(config: &Config, project_root: &Path) -> Result<Outcome, Error> {
    let steps = check_commands(config, project_root);

    // Ensure the output directory exists before any builder writes into it.
    // ubc writes to needs_json's parent; sphinx writes into the same outdir.
    ensure_output_dir(config)?;

    let mut findings: Vec<Value> = Vec::new();
    for step in &steps {
        let status = run_step(&step.command)?;
        if !status.success() {
            findings.push(step_finding(&step.name, &step.command.program, &status));
        }
    }

    let needs_json_exists = config.needs_json.exists();

    // All green but no needs.json: the adapter mis-ran — surface as a tool error.
    if findings.is_empty() && !needs_json_exists {
        return Err(Error::Tool {
            message: format!(
                "check passed but did not produce {}",
                config.needs_json.display()
            ),
        });
    }

    let mut payload = Map::new();
    if findings.is_empty() {
        // Clean: explicit empty findings array plus the fresh needs.json path.
        payload.insert("findings".to_string(), Value::Array(Vec::new()));
        payload.insert(
            "needs_json".to_string(),
            Value::String(config.needs_json.to_string_lossy().into_owned()),
        );
        Ok(Outcome::clean(payload))
    } else {
        payload.insert("findings".to_string(), Value::Array(findings));
        // Report needs.json only when it was actually produced.
        if needs_json_exists {
            payload.insert(
                "needs_json".to_string(),
                Value::String(config.needs_json.to_string_lossy().into_owned()),
            );
        }
        Ok(Outcome::failed(payload))
    }
}

/// Create the directory the builders write into, if missing. Both builders land
/// `needs.json` under `needs_json`'s parent (for sphinx, the same `outdir`).
fn ensure_output_dir(config: &Config) -> Result<(), Error> {
    let dir = needs_json_outdir(&config.needs_json);
    create_dir(&dir)
}

fn create_dir(dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::Tool {
        message: format!("cannot create output directory {}: {e}", dir.display()),
    })
}

/// Spawn one step, draining its stdout to pds's stderr, and await it. A
/// non-spawnable program is an [`Error::Tool`] naming the program.
fn run_step(cmd: &BuildCommand) -> Result<std::process::ExitStatus, Error> {
    let mut child = Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| Error::Tool {
            message: format!("cannot run {:?}: {e}", cmd.program),
        })?;

    // Drain child stdout into our stderr so it never reaches our stdout.
    if let Some(mut out) = child.stdout.take() {
        let mut err = std::io::stderr();
        let _ = std::io::copy(&mut out, &mut err);
    }

    child.wait().map_err(|e| Error::Tool {
        message: format!("{:?} could not be awaited: {e}", cmd.program),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::config::IssueBackend;

    fn config_with(builder: Builder, needs_json: &str, spec_dir: &str) -> Config {
        Config {
            spec_dir: PathBuf::from(spec_dir),
            builder,
            issue_backend: IssueBackend::SphinxNeeds,
            issue_doc: None,
            roles: HashMap::new(),
            needs_json: PathBuf::from(needs_json),
            sphinx_command: vec![
                "uv".to_string(),
                "run".to_string(),
                "sphinx-build".to_string(),
            ],
        }
    }

    #[test]
    fn ubc_check_is_two_steps_in_order() {
        let cfg = config_with(
            Builder::Ubc,
            "/proj/spec/_build/needs/needs.json",
            "/proj/spec",
        );
        let steps = check_commands(&cfg, Path::new("/proj"));
        assert_eq!(steps.len(), 2);

        // Step 1: ubc check.
        assert_eq!(steps[0].name, "ubc-check");
        assert_eq!(steps[0].command.program, "ubc");
        assert_eq!(steps[0].command.args, vec!["check"]);
        assert_eq!(steps[0].command.cwd, PathBuf::from("/proj"));

        // Step 2: ubc build needs --outpath <needs_json>.
        assert_eq!(steps[1].name, "build");
        assert_eq!(steps[1].command.program, "ubc");
        assert_eq!(
            steps[1].command.args,
            vec![
                "build",
                "needs",
                "--outpath",
                "/proj/spec/_build/needs/needs.json"
            ]
        );
        assert_eq!(steps[1].command.cwd, PathBuf::from("/proj"));
    }

    #[test]
    fn sphinx_check_is_single_gating_build_step() {
        let cfg = config_with(
            Builder::SphinxBuild,
            "/proj/spec/_build/needs/needs.json",
            "/proj/spec",
        );
        let steps = check_commands(&cfg, Path::new("/proj"));
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[0].command.program, "uv");
        assert_eq!(
            steps[0].command.args,
            vec![
                "run",
                "sphinx-build",
                "-W",
                "-b",
                "needs",
                "/proj/spec",
                "/proj/spec/_build/needs"
            ]
        );
        assert_eq!(steps[0].command.cwd, PathBuf::from("/proj"));
    }

    #[test]
    fn sphinx_check_step_carries_the_gate_flag() {
        let cfg = config_with(
            Builder::SphinxBuild,
            "/p/spec/_build/needs/needs.json",
            "/p/spec",
        );
        let steps = check_commands(&cfg, Path::new("/p"));
        assert!(
            steps[0].command.args.iter().any(|a| a == "-W"),
            "sphinx check must be gating (-W present)"
        );
    }
}
