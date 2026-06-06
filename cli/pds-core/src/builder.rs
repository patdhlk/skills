//! Builder adapter for the `pds build` verb.
//!
//! `pds build` produces a *fresh* `needs.json` at the configured path by
//! invoking the project's configured builder ([`Builder::Ubc`] or
//! [`Builder::SphinxBuild`]). It does **not** run the strict gate — that is
//! `pds check`'s job. The child's stdout and stderr are passed through to
//! pds's stderr so pds's own stdout stays reserved for the one JSON object.
//!
//! Command construction is split out as a pure function ([`build_command`])
//! so it can be tested without spawning a process.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

use crate::config::{Builder, Config};
use crate::error::Error;
use crate::outcome::Outcome;

/// A fully-resolved builder invocation: which program, with which arguments,
/// run from which working directory. Pure data so command construction is
/// testable without touching the process table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCommand {
    /// The program to spawn (first element; e.g. `"ubc"` or `"uv"`).
    pub program: String,
    /// Arguments passed to `program`, in order.
    pub args: Vec<String>,
    /// Working directory for the child (always the project root).
    pub cwd: PathBuf,
}

/// Construct the builder invocation for `config`, run from `project_root`.
///
/// - [`Builder::Ubc`] → `ubc build needs --outpath <needs_json>`.
/// - [`Builder::SphinxBuild`] → `<sphinx_command...> -b needs <spec_dir>
///   <needs_json parent>` (the sphinx "needs" builder writes `needs.json`
///   into its output directory, so the outdir is `needs_json`'s parent).
///   No `-W`: the build verb is non-gating.
pub fn build_command(config: &Config, project_root: &Path) -> BuildCommand {
    match config.builder {
        Builder::Ubc => BuildCommand {
            program: "ubc".to_string(),
            args: vec![
                "build".to_string(),
                "needs".to_string(),
                "--outpath".to_string(),
                config.needs_json.to_string_lossy().into_owned(),
            ],
            cwd: project_root.to_path_buf(),
        },
        Builder::SphinxBuild => {
            let (program, rest) = config
                .sphinx_command
                .split_first()
                .expect("sphinx_command is validated non-empty at config load");
            let outdir = needs_json_outdir(&config.needs_json);
            let mut args: Vec<String> = rest.to_vec();
            args.push("-b".to_string());
            args.push("needs".to_string());
            args.push(config.spec_dir.to_string_lossy().into_owned());
            args.push(outdir.to_string_lossy().into_owned());
            BuildCommand {
                program: program.clone(),
                args,
                cwd: project_root.to_path_buf(),
            }
        }
    }
}

/// Run the configured builder and classify the result per the `pds build`
/// contract.
///
/// Creates `needs_json`'s parent directory if missing, then spawns the builder
/// from the project root with the child's stdout and stderr both routed to
/// pds's stderr (inherited), keeping pds's stdout free for the JSON envelope.
///
/// Outcomes:
/// - exit 0 **and** `needs_json` exists → [`Outcome::clean`] with payload
///   `{"needs_json": "<absolute path>"}`.
/// - exit non-zero → [`Outcome::failed`] with a single build finding.
/// - exit 0 but `needs_json` missing → [`Error::Tool`] (adapter mis-ran).
/// - program cannot be spawned → [`Error::Tool`] naming the program.
pub fn run_build(config: &Config, project_root: &Path) -> Result<Outcome, Error> {
    let cmd = build_command(config, project_root);

    // Ensure the output directory exists before the builder writes into it.
    if let Some(parent) = config.needs_json.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Tool {
            message: format!(
                "cannot create needs.json output directory {}: {e}",
                parent.display()
            ),
        })?;
    }

    // stderr is inherited (child stderr → pds stderr). stdout is piped and
    // copied to pds's stderr so the child can never pollute pds's stdout.
    let mut child = Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| Error::Tool {
            message: format!("cannot run builder {:?}: {e}", cmd.program),
        })?;

    // Drain child stdout into our stderr so it never reaches our stdout.
    if let Some(mut out) = child.stdout.take() {
        let mut err = std::io::stderr();
        let _ = std::io::copy(&mut out, &mut err);
    }

    let status = child.wait().map_err(|e| Error::Tool {
        message: format!("builder {:?} could not be awaited: {e}", cmd.program),
    })?;

    if !status.success() {
        return Ok(Outcome::failed(build_failure_payload(
            &cmd.program,
            &status,
        )));
    }

    // Exit 0: the fresh needs.json must now exist.
    if !config.needs_json.exists() {
        return Err(Error::Tool {
            message: format!(
                "builder {:?} exited 0 but did not produce {}",
                cmd.program,
                config.needs_json.display()
            ),
        });
    }

    let mut payload = Map::new();
    payload.insert(
        "needs_json".to_string(),
        Value::String(config.needs_json.to_string_lossy().into_owned()),
    );
    Ok(Outcome::clean(payload))
}

/// Build the `{"findings": [...]}` payload for a non-zero builder exit.
fn build_failure_payload(program: &str, status: &std::process::ExitStatus) -> Map<String, Value> {
    let status_desc = match status.code() {
        Some(code) => code.to_string(),
        None => "signal".to_string(),
    };
    let finding = json!({
        "check": "build",
        "severity": "error",
        "need": Value::Null,
        "message": format!("{program} exited with status {status_desc}"),
    });
    let mut payload = Map::new();
    payload.insert("findings".to_string(), Value::Array(vec![finding]));
    payload
}

/// The directory the sphinx "needs" builder should write into: the parent of
/// `needs_json`, or the path itself if it has no parent (defensive — config
/// always yields an absolute path with a parent).
fn needs_json_outdir(needs_json: &Path) -> PathBuf {
    needs_json
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| needs_json.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::config::IssueBackend;

    /// Minimal Config with the builder and paths needed for command tests.
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
    fn ubc_command_is_build_needs_outpath() {
        let cfg = config_with(
            Builder::Ubc,
            "/proj/spec/_build/needs/needs.json",
            "/proj/spec",
        );
        let cmd = build_command(&cfg, Path::new("/proj"));
        assert_eq!(cmd.program, "ubc");
        assert_eq!(
            cmd.args,
            vec![
                "build",
                "needs",
                "--outpath",
                "/proj/spec/_build/needs/needs.json"
            ]
        );
        assert_eq!(cmd.cwd, PathBuf::from("/proj"));
    }

    #[test]
    fn sphinx_command_splits_program_and_passes_needs_builder_with_outdir() {
        let cfg = config_with(
            Builder::SphinxBuild,
            "/proj/spec/_build/needs/needs.json",
            "/proj/spec",
        );
        let cmd = build_command(&cfg, Path::new("/proj"));
        // First element of sphinx_command is the program; the rest are leading args.
        assert_eq!(cmd.program, "uv");
        assert_eq!(
            cmd.args,
            vec![
                "run",
                "sphinx-build",
                "-b",
                "needs",
                "/proj/spec",
                // outdir = parent of needs_json, NOT the needs.json file itself
                "/proj/spec/_build/needs"
            ]
        );
        assert_eq!(cmd.cwd, PathBuf::from("/proj"));
    }

    #[test]
    fn sphinx_command_has_no_dash_w_flag() {
        let cfg = config_with(
            Builder::SphinxBuild,
            "/proj/spec/_build/needs/needs.json",
            "/proj/spec",
        );
        let cmd = build_command(&cfg, Path::new("/proj"));
        assert!(
            !cmd.args.iter().any(|a| a == "-W"),
            "build verb must be non-gating; no -W"
        );
    }

    #[test]
    fn custom_sphinx_command_program_is_first_element() {
        let mut cfg = config_with(Builder::SphinxBuild, "/proj/out/needs.json", "/proj/docs");
        cfg.sphinx_command = vec!["/abs/fake-sphinx.sh".to_string()];
        let cmd = build_command(&cfg, Path::new("/proj"));
        assert_eq!(cmd.program, "/abs/fake-sphinx.sh");
        assert_eq!(cmd.args, vec!["-b", "needs", "/proj/docs", "/proj/out"]);
    }
}
