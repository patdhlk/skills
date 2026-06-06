use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use pds_core::{Error, Outcome, Project};
use serde_json::{Map, Value, json};

#[derive(Parser)]
#[command(
    name = "pds",
    version,
    about = "Gate-and-query CLI for sphinx-needs repos"
)]
struct Cli {
    /// Use this ubproject.toml instead of discovering one from the cwd.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build the needs corpus.
    Build,
    /// Check the corpus for violations.
    Check,
}

impl Commands {
    fn verb(&self) -> &'static str {
        match self {
            Commands::Build => "build",
            Commands::Check => "check",
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let verb = cli.command.verb();
    match run(&cli) {
        // Clean run => exit 0; corpus violations => exit 1. Both are successful runs.
        Ok(outcome) => {
            let failed = outcome.is_failed();
            emit_success(verb, outcome.into_payload());
            if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        // Tool/config problems => exit 2.
        Err(err) => {
            emit_error(verb, &err);
            ExitCode::from(err.exit_code())
        }
    }
}

/// Resolve the project, then dispatch the verb. Verbs are honest placeholders
/// until Tasks 3/4 wire in build/check logic.
fn run(cli: &Cli) -> Result<Outcome, Error> {
    let _project = resolve_project(cli)?;
    Err(Error::Tool {
        message: format!("not implemented: {}", cli.command.verb()),
    })
}

fn resolve_project(cli: &Cli) -> Result<Project, Error> {
    match &cli.config {
        Some(path) => Project::from_config_path(path),
        None => {
            let cwd = std::env::current_dir().map_err(|e| Error::Config {
                message: format!("cannot read current directory: {e}"),
            })?;
            Project::discover(&cwd)
        }
    }
}

/// Print the single success JSON object on stdout. The payload is already a JSON
/// object, so the envelope and payload merge without any shape check.
fn emit_success(verb: &str, payload: Map<String, Value>) {
    let mut obj = json!({ "schema": 1, "verb": verb });
    if let Some(map) = obj.as_object_mut() {
        map.extend(payload);
    }
    println!("{obj}");
}

/// Print the JSON error envelope on stdout and a human line on stderr.
fn emit_error(verb: &str, err: &Error) {
    let obj = json!({
        "schema": 1,
        "verb": verb,
        "error": { "kind": err.kind(), "message": err.to_string() },
    });
    println!("{obj}");
    eprintln!("pds {verb}: {err}");
}
