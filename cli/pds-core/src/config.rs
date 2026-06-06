//! Config parsing and validation for `ubproject.toml`.
//!
//! [`Config::load`] reads the project's `ubproject.toml`, applies defaults,
//! and validates all values. Unknown keys in `[tool.patdhlk-skills]` are
//! silently ignored so future keys don't break old binaries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Error;
use crate::project::Project;

// ---------------------------------------------------------------------------
// Public enums
// ---------------------------------------------------------------------------

/// Which Sphinx/needs builder to invoke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Builder {
    /// The `ubc` binary (faster, local cache).
    Ubc,
    /// The standard `sphinx-build` invocation.
    SphinxBuild,
}

/// Where issues are tracked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueBackend {
    /// Local sphinx-needs corpus (default).
    SphinxNeeds,
    /// GitHub Issues.
    Github,
}

// ---------------------------------------------------------------------------
// Public Config type
// ---------------------------------------------------------------------------

/// Resolved, validated configuration loaded from `ubproject.toml`.
#[derive(Debug, Clone)]
pub struct Config {
    /// Absolute path to the spec source directory.
    pub spec_dir: PathBuf,
    /// Which builder to use.
    pub builder: Builder,
    /// Where issues are tracked.
    pub issue_backend: IssueBackend,
    /// Absolute path to the issue RST document (optional).
    pub issue_doc: Option<PathBuf>,
    /// Role→directive mapping (empty when absent).
    pub roles: HashMap<String, String>,
    /// Absolute path to the needs JSON output file.
    pub needs_json: PathBuf,
    /// Command used to invoke Sphinx (non-empty).
    pub sphinx_command: Vec<String>,
}

impl Config {
    /// Load and validate configuration from the project's `ubproject.toml`.
    ///
    /// Whole `[tool.patdhlk-skills]` table absent → all defaults apply.
    /// Unreadable or TOML-syntax-invalid file → `Error::Config`.
    pub fn load(project: &Project) -> Result<Config, Error> {
        let raw = std::fs::read_to_string(&project.config_path).map_err(|e| Error::Config {
            message: format!("cannot read {}: {e}", project.config_path.display()),
        })?;

        let doc: RawDoc = toml::from_str(&raw).map_err(|e| Error::Config {
            message: format!("TOML parse error in {}: {e}", project.config_path.display()),
        })?;

        Config::from_raw(doc, &project.root)
    }

    fn from_raw(doc: RawDoc, root: &Path) -> Result<Config, Error> {
        let tool = doc.tool.and_then(|t| t.patdhlk_skills);
        let raw_srcdir = doc.project.and_then(|p| p.srcdir);
        let declared_directives: Vec<String> = doc
            .needs
            .map(|n| n.types.into_iter().map(|t| t.directive).collect())
            .unwrap_or_default();

        // spec_dir: tool > project.srcdir > "spec"
        let spec_dir_rel = tool
            .as_ref()
            .and_then(|t| t.spec_dir.clone())
            .or(raw_srcdir)
            .unwrap_or_else(|| "spec".to_string());
        let spec_dir = absolutize_path(root, &spec_dir_rel);

        // builder
        let builder = match tool.as_ref().and_then(|t| t.builder.as_deref()) {
            None | Some("sphinx-build") => Builder::SphinxBuild,
            Some("ubc") => Builder::Ubc,
            Some(other) => {
                return Err(Error::Config {
                    message: format!(
                        "unknown builder {other:?}; expected \"ubc\" or \"sphinx-build\""
                    ),
                });
            }
        };

        // issue_backend
        let issue_backend = match tool.as_ref().and_then(|t| t.issue_backend.as_deref()) {
            None | Some("sphinx-needs") => IssueBackend::SphinxNeeds,
            Some("github") => IssueBackend::Github,
            Some(other) => {
                return Err(Error::Config {
                    message: format!(
                        "unknown issue_backend {other:?}; expected \"sphinx-needs\" or \"github\""
                    ),
                });
            }
        };

        // issue_doc
        let issue_doc = tool
            .as_ref()
            .and_then(|t| t.issue_doc.as_deref())
            .map(|p| absolutize_path(root, p));

        // roles
        let roles: HashMap<String, String> = tool
            .as_ref()
            .and_then(|t| t.roles.clone())
            .unwrap_or_default();

        // Fail-on-drift: every directive value in roles must appear in [[needs.types]]
        if !roles.is_empty() {
            let mut drift: Vec<(String, String)> = roles
                .iter()
                .filter(|(_, directive)| !declared_directives.contains(directive))
                .map(|(role, directive)| (role.clone(), directive.clone()))
                .collect();
            if !drift.is_empty() {
                // Sort for deterministic error messages.
                drift.sort_by(|a, b| a.0.cmp(&b.0));
                let pairs: Vec<String> = drift
                    .iter()
                    .map(|(r, d)| format!("role {r:?} -> directive {d:?}"))
                    .collect();
                return Err(Error::Config {
                    message: format!(
                        "roles reference undeclared [[needs.types]] directives: {}",
                        pairs.join(", ")
                    ),
                });
            }
        }

        // gate.needs_json: from table or default <spec_dir>/_build/needs/needs.json
        let needs_json = tool
            .as_ref()
            .and_then(|t| t.gate.as_ref())
            .and_then(|g| g.needs_json.as_deref())
            .map(|p| absolutize_path(root, p))
            .unwrap_or_else(|| spec_dir.join("_build/needs/needs.json"));

        // gate.sphinx_command: from table or default
        let sphinx_command = match tool
            .as_ref()
            .and_then(|t| t.gate.as_ref())
            .and_then(|g| g.sphinx_command.clone())
        {
            None => vec![
                "uv".to_string(),
                "run".to_string(),
                "sphinx-build".to_string(),
            ],
            Some(cmd) if cmd.is_empty() => {
                return Err(Error::Config {
                    message: "gate.sphinx_command must not be an empty array".to_string(),
                });
            }
            Some(cmd) => cmd,
        };

        Ok(Config {
            spec_dir,
            builder,
            issue_backend,
            issue_doc,
            roles,
            needs_json,
            sphinx_command,
        })
    }
}

// ---------------------------------------------------------------------------
// Raw serde types (private)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawDoc {
    project: Option<RawProject>,
    needs: Option<RawNeeds>,
    tool: Option<RawTool>,
}

#[derive(Deserialize)]
struct RawProject {
    srcdir: Option<String>,
}

#[derive(Deserialize)]
struct RawNeeds {
    #[serde(default)]
    types: Vec<RawNeedsType>,
}

#[derive(Deserialize)]
struct RawNeedsType {
    directive: String,
}

#[derive(Deserialize)]
struct RawTool {
    #[serde(rename = "patdhlk-skills")]
    patdhlk_skills: Option<RawPatdhlkSkills>,
}

/// `[tool.patdhlk-skills]` — only the keys we care about; unknown keys ignored.
#[derive(Deserialize, Default)]
struct RawPatdhlkSkills {
    spec_dir: Option<String>,
    builder: Option<String>,
    issue_backend: Option<String>,
    issue_doc: Option<String>,
    roles: Option<HashMap<String, String>>,
    gate: Option<RawGate>,
}

#[derive(Deserialize)]
struct RawGate {
    needs_json: Option<String>,
    sphinx_command: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn absolutize_path(root: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    /// Write a temp ubproject.toml and return a Project pointing at it.
    fn make_project(content: &str) -> (tempfile::TempDir, Project) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let config_path = root.join("ubproject.toml");
        std::fs::write(&config_path, content).unwrap();
        let project = Project { root, config_path };
        (tmp, project)
    }

    // ------------------------------------------------------------------
    // Full realistic config (modelled on this repo's ubproject.toml)
    // ------------------------------------------------------------------

    #[test]
    fn full_realistic_config_parses_correctly() {
        let toml = r#"
[project]
name = "patdhlk-skills"
srcdir = "spec"

[[needs.types]]
directive = "issue"

[[needs.types]]
directive = "feat"

[[needs.types]]
directive = "arch-decision"

[[needs.types]]
directive = "term"

[[needs.types]]
directive = "test"

[tool.patdhlk-skills]
issue_backend = "sphinx-needs"
spec_dir = "spec"
builder = "ubc"
issue_doc = "spec/issues/index.rst"

[tool.patdhlk-skills.roles]
issue = "issue"
feature = "feat"
decision = "arch-decision"
term = "term"
test = "test"

[tool.patdhlk-skills.gate]
needs_json = "spec/_build/needs/needs.json"
sphinx_command = ["uv", "run", "sphinx-build"]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();

        assert_eq!(cfg.builder, Builder::Ubc);
        assert_eq!(cfg.issue_backend, IssueBackend::SphinxNeeds);
        assert!(cfg.spec_dir.is_absolute());
        assert!(cfg.spec_dir.ends_with("spec"));
        assert!(cfg.issue_doc.is_some());
        assert!(
            cfg.issue_doc
                .as_ref()
                .unwrap()
                .ends_with("spec/issues/index.rst")
        );
        assert_eq!(cfg.roles.get("issue").map(String::as_str), Some("issue"));
        assert_eq!(cfg.roles.get("feature").map(String::as_str), Some("feat"));
        assert_eq!(
            cfg.roles.get("decision").map(String::as_str),
            Some("arch-decision")
        );
        assert!(cfg.needs_json.is_absolute());
        assert!(cfg.needs_json.ends_with("spec/_build/needs/needs.json"));
        assert_eq!(cfg.sphinx_command, vec!["uv", "run", "sphinx-build"]);
    }

    // ------------------------------------------------------------------
    // All-defaults: missing [tool.patdhlk-skills] table
    // ------------------------------------------------------------------

    #[test]
    fn missing_tool_table_applies_all_defaults() {
        let toml = r#"
[project]
name = "minimal"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();

        assert_eq!(cfg.builder, Builder::SphinxBuild);
        assert_eq!(cfg.issue_backend, IssueBackend::SphinxNeeds);
        assert!(cfg.spec_dir.is_absolute());
        assert!(cfg.spec_dir.ends_with("spec"));
        assert!(cfg.issue_doc.is_none());
        assert!(cfg.roles.is_empty());
        // needs_json defaults to <spec_dir>/_build/needs/needs.json
        assert!(cfg.needs_json.ends_with("spec/_build/needs/needs.json"));
        assert_eq!(cfg.sphinx_command, vec!["uv", "run", "sphinx-build"]);
    }

    // ------------------------------------------------------------------
    // spec_dir fallback chain: tool > project.srcdir > "spec"
    // ------------------------------------------------------------------

    #[test]
    fn spec_dir_from_tool_beats_project_srcdir() {
        let toml = r#"
[project]
srcdir = "from_project"

[tool.patdhlk-skills]
spec_dir = "from_tool"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.spec_dir.ends_with("from_tool"));
    }

    #[test]
    fn spec_dir_falls_back_to_project_srcdir() {
        let toml = r#"
[project]
srcdir = "from_project"

[tool.patdhlk-skills]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.spec_dir.ends_with("from_project"));
    }

    #[test]
    fn spec_dir_falls_back_to_spec_literal() {
        let toml = r#"
[project]
name = "no-srcdir"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.spec_dir.ends_with("spec"));
    }

    // ------------------------------------------------------------------
    // spec_dir is absolute
    // ------------------------------------------------------------------

    #[test]
    fn spec_dir_is_absolute() {
        let toml = r#"
[tool.patdhlk-skills]
spec_dir = "docs"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.spec_dir.is_absolute(), "spec_dir must be absolute");
    }

    // ------------------------------------------------------------------
    // needs_json default derivation
    // ------------------------------------------------------------------

    #[test]
    fn needs_json_defaults_relative_to_spec_dir() {
        let toml = r#"
[tool.patdhlk-skills]
spec_dir = "docs"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(
            cfg.needs_json.ends_with("docs/_build/needs/needs.json"),
            "got: {}",
            cfg.needs_json.display()
        );
    }

    #[test]
    fn needs_json_from_gate_table_is_absolute() {
        let toml = r#"
[tool.patdhlk-skills]
spec_dir = "spec"

[tool.patdhlk-skills.gate]
needs_json = "custom/_build/needs.json"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.needs_json.is_absolute());
        assert!(
            cfg.needs_json.ends_with("custom/_build/needs.json"),
            "got: {}",
            cfg.needs_json.display()
        );
    }

    // ------------------------------------------------------------------
    // Validation errors
    // ------------------------------------------------------------------

    #[test]
    fn bad_builder_is_config_error_naming_the_value() {
        let toml = r#"
[tool.patdhlk-skills]
builder = "make"
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("make"),
            "error message should name the bad value, got: {msg}"
        );
    }

    #[test]
    fn bad_issue_backend_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills]
issue_backend = "jira"
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("jira"),
            "error message should name the bad value, got: {msg}"
        );
    }

    #[test]
    fn role_drift_is_config_error_listing_offenders() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.roles]
issue = "issue"
decision = "arch-decision"
"#;
        // "arch-decision" is not declared in [[needs.types]], so this is drift.
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("arch-decision"),
            "error must name the offending directive, got: {msg}"
        );
        assert!(
            msg.contains("decision"),
            "error must name the offending role, got: {msg}"
        );
    }

    #[test]
    fn roles_with_no_needs_types_is_drift_error() {
        // No [[needs.types]] at all but roles exist → drift.
        let toml = r#"
[tool.patdhlk-skills.roles]
issue = "issue"
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("issue"),
            "error must name the offending directive, got: {msg}"
        );
    }

    #[test]
    fn empty_sphinx_command_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.gate]
sphinx_command = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("sphinx_command"));
    }

    #[test]
    fn toml_syntax_error_is_config_error() {
        let toml = "not = valid toml ][";
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        // Message should mention the file.
        let msg = err.to_string();
        assert!(
            msg.contains("TOML parse error") || msg.contains("ubproject.toml"),
            "expected parse error context, got: {msg}"
        );
    }

    #[test]
    fn unreadable_config_file_is_config_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // config_path points to a non-existent file
        let config_path = root.join("ubproject.toml");
        let project = Project { root, config_path };
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
    }

    // ------------------------------------------------------------------
    // Builder / backend enum defaults
    // ------------------------------------------------------------------

    #[test]
    fn builder_defaults_to_sphinx_build() {
        let (_tmp, project) = make_project("[project]\nname = \"x\"");
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.builder, Builder::SphinxBuild);
    }

    #[test]
    fn builder_ubc_parses() {
        let toml = "[tool.patdhlk-skills]\nbuilder = \"ubc\"";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.builder, Builder::Ubc);
    }

    #[test]
    fn builder_sphinx_build_explicit_parses() {
        let toml = "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.builder, Builder::SphinxBuild);
    }

    #[test]
    fn backend_github_parses() {
        let toml = "[tool.patdhlk-skills]\nissue_backend = \"github\"";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.issue_backend, IssueBackend::Github);
    }

    #[test]
    fn backend_defaults_to_sphinx_needs() {
        let (_tmp, project) = make_project("[project]\nname = \"x\"");
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.issue_backend, IssueBackend::SphinxNeeds);
    }

    // ------------------------------------------------------------------
    // issue_doc optional / absolute
    // ------------------------------------------------------------------

    #[test]
    fn issue_doc_is_absolute_when_present() {
        let toml = r#"
[tool.patdhlk-skills]
issue_doc = "spec/issues/index.rst"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let doc = cfg.issue_doc.unwrap();
        assert!(doc.is_absolute());
        assert!(doc.ends_with("spec/issues/index.rst"));
    }

    #[test]
    fn issue_doc_absent_is_none() {
        let (_tmp, project) = make_project("[project]\nname = \"x\"");
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.issue_doc.is_none());
    }

    // ------------------------------------------------------------------
    // Unknown keys in [tool.patdhlk-skills] are ignored
    // ------------------------------------------------------------------

    #[test]
    fn unknown_keys_in_tool_table_are_ignored() {
        let toml = r#"
[tool.patdhlk-skills]
builder = "ubc"
future_key_from_task_5 = "some_value"
another_future = 42
"#;
        let (_tmp, project) = make_project(toml);
        // Must not error out on unknown keys.
        let result = Config::load(&project);
        assert!(
            result.is_ok(),
            "unknown keys should be ignored, got: {:?}",
            result
        );
    }

    // ------------------------------------------------------------------
    // sphinx_command non-default
    // ------------------------------------------------------------------

    #[test]
    fn custom_sphinx_command_parses() {
        let toml = r#"
[tool.patdhlk-skills.gate]
sphinx_command = ["python", "-m", "sphinx"]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.sphinx_command, vec!["python", "-m", "sphinx"]);
    }

    // ------------------------------------------------------------------
    // All roles valid passes without error
    // ------------------------------------------------------------------

    #[test]
    fn all_roles_valid_passes() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[[needs.types]]
directive = "feat"

[tool.patdhlk-skills.roles]
issue = "issue"
feature = "feat"
"#;
        let (_tmp, project) = make_project(toml);
        let result = Config::load(&project);
        assert!(result.is_ok());
    }
}
