use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use pds_core::{Config, Error, Outcome, Project};
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
    /// Lint need bodies for substance (required sections, weasel words, …).
    Lint,
    /// Report per-status counts over the issue backlog.
    Status,
    /// Report the next actionable (ready-for-agent) issue.
    Next,
    /// Rank needs by relevance to a query; exit 0 even with zero hits (1 = build failure, 2 = config/tool error).
    Search {
        /// The query text.
        query: String,
    },
    /// Pre-filing duplicate gate: exit 1 when an issue-typed hit reaches the threshold.
    Dedup {
        /// The candidate issue text (title plus body draft).
        candidate: String,
        /// Override the configured similarity threshold (a ratio in (0, 1]).
        #[arg(long, value_name = "RATIO")]
        threshold: Option<f64>,
    },
    /// Check verdict coverage: missing / failing / stale / malformed (exit 1 on any).
    VerdictCheck,
}

impl Commands {
    fn verb(&self) -> &'static str {
        match self {
            Commands::Build => "build",
            Commands::Check => "check",
            Commands::Lint => "lint",
            Commands::Status => "status",
            Commands::Next => "next",
            Commands::Search { .. } => "search",
            Commands::Dedup { .. } => "dedup",
            Commands::VerdictCheck => "verdict-check",
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

/// Resolve the project and load its config, then dispatch the verb.
/// `build` runs the real builder adapter; `check` runs the strict gate
/// (fresh needs.json plus fail-closed diagnostics).
fn run(cli: &Cli) -> Result<Outcome, Error> {
    let project = resolve_project(cli)?;
    let config = Config::load(&project)?;
    match &cli.command {
        Commands::Build => pds_core::run_build(&config, &project.root),
        Commands::Check => pds_core::run_check(&config, &project.root),
        Commands::Lint => pds_core::run_lint(&config, &project.root),
        Commands::Status => pds_core::run_status(&config, &project.root),
        Commands::Next => pds_core::run_next(&config, &project.root),
        Commands::Search { query } => pds_core::run_search(&config, &project.root, query),
        Commands::Dedup {
            candidate,
            threshold,
        } => pds_core::run_dedup(&config, &project.root, candidate, *threshold),
        Commands::VerdictCheck => pds_core::run_verdict_check(&config, &project.root),
    }
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
