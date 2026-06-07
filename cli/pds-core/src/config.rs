//! Config parsing and validation for `ubproject.toml`.
//!
//! [`Config::load`] reads the project's `ubproject.toml`, applies defaults,
//! and validates all values. Unknown keys in `[tool.patdhlk-skills]` are
//! silently ignored so future keys don't break old binaries.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

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

/// Validated per-directive body-length bounds parsed from `[tool.patdhlk-skills.lint]`.
///
/// Both `min` (from `nontrivial_body`) and `max` (from `max_body_length`) are optional
/// independently; the directive key must appear in `[[needs.types]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintBodyLength {
    /// Minimum body length in characters (> 0).
    pub min: Option<usize>,
    /// Maximum body length in characters (> 0).
    pub max: Option<usize>,
}

/// Validated weasel-word rule from `[tool.patdhlk-skills.lint]`.
///
/// Present only when the `weasel_words` key is in the lint table.
/// Both fields are guaranteed non-empty (each `String` is also non-empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintWeaselWords {
    /// Words that trigger the check (non-empty; each element non-empty).
    pub words: Vec<String>,
    /// Directive names the check applies to (non-empty; each element non-empty).
    pub directives: Vec<String>,
}

/// Validated unenumerated-quantifier rule from `[tool.patdhlk-skills.lint]`.
///
/// Present only when the `unenumerated_quantifiers` key is in the lint table.
/// Both fields are guaranteed non-empty (each `String` is also non-empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintUnenumeratedQuantifiers {
    /// Quantifier strings that trigger the check (non-empty; each element non-empty).
    pub quantifiers: Vec<String>,
    /// Directive names the check applies to (non-empty; each element non-empty).
    pub directives: Vec<String>,
}

/// Validated `[tool.patdhlk-skills.verdicts]` table (ADR_0016 + the
/// ISSUE_0014 `statuses` scoping amendment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictsConfig {
    /// Need type → required rubric name. Non-empty; every key is a declared
    /// directive, every value a declared rubric.
    pub require: HashMap<String, String>,
    /// Statuses in which a verdict is demanded. `None` = all non-exempt
    /// statuses (including statusless needs). Non-empty when `Some`.
    pub statuses: Option<Vec<String>>,
}

/// Validated lint configuration from `[tool.patdhlk-skills.lint]`.
///
/// `None` on any field means that rule is disabled.  The table may be present
/// with all keys absent — that is valid and means lint runs but all rules are
/// off (no findings).
///
/// Every directive name referenced by any rule is guaranteed to appear in the
/// declared `[[needs.types]]`; a violation is a `Error::Config` (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintConfig {
    /// Per-directive required section lead-ins.
    ///
    /// Key: directive name. Value: list of bold-text lead-ins that must appear
    /// in the body (e.g. `["Context", "Decision", "Consequences"]` for
    /// `arch-decision`). Both the map and every inner `Vec` are guaranteed
    /// non-empty (all directive names are in `[[needs.types]]`).
    pub required_sections: Option<HashMap<String, Vec<String>>>,

    /// Per-directive body-length bounds.
    ///
    /// Key: directive name. Value: min/max pair (at least one is `Some`).
    /// The map is non-empty when `Some`. All directive names are in
    /// `[[needs.types]]`.
    pub body_length: Option<HashMap<String, LintBodyLength>>,

    /// Weasel-word rule.
    pub weasel_words: Option<LintWeaselWords>,

    /// Unenumerated-quantifier rule.
    pub unenumerated_quantifiers: Option<LintUnenumeratedQuantifiers>,
}

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
    /// Statuses whose needs are exempt from all corpus checks (lint, verdict-check, …).
    ///
    /// Defaults to `["done", "wontfix"]` when the key is absent from
    /// `[tool.patdhlk-skills.gate]`.  An explicitly empty list is legal and
    /// means nothing is exempt.  Shared plumbing so every future
    /// check verb reads the same policy (ISSUE_0018 mechanism).
    pub exempt_statuses: Vec<String>,
    /// Lint rule configuration (`None` when `[tool.patdhlk-skills.lint]` is absent).
    pub lint: Option<LintConfig>,
    /// Similarity threshold for `pds dedup` (0, 1], from
    /// `[tool.patdhlk-skills.dedup]`; defaults to
    /// [`crate::retrieval::DEFAULT_THRESHOLD`] when absent.
    pub dedup_threshold: f64,
    /// Declared rubrics: name → axis list (`[tool.patdhlk-skills.rubrics.<name>]`).
    /// Empty when no rubrics are declared. Axes are non-empty, unique strings.
    pub rubrics: HashMap<String, Vec<String>>,
    /// Verdict requirements (`None` when the table is absent — verdict-check
    /// is then a clean no-op, the lint activation model).
    pub verdicts: Option<VerdictsConfig>,
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
        let raw_srcdir = doc.project.and_then(|p| p.srcdir);

        // Build an O(1) lookup set from [[needs.types]] before consuming `doc.needs`.
        let declared_directives: HashSet<String> = doc
            .needs
            .map(|n| n.types.into_iter().map(|t| t.directive).collect())
            .unwrap_or_default();

        // Destructure the tool table once so all fields can be moved/borrowed freely.
        let RawPatdhlkSkills {
            spec_dir: raw_spec_dir,
            builder: raw_builder,
            issue_backend: raw_issue_backend,
            issue_doc: raw_issue_doc,
            roles: raw_roles,
            gate: raw_gate,
            lint: raw_lint,
            dedup: raw_dedup,
            rubrics: raw_rubrics,
            verdicts: raw_verdicts,
        } = doc.tool.and_then(|t| t.patdhlk_skills).unwrap_or_default();

        // spec_dir: tool > project.srcdir > "spec"
        let spec_dir_rel = raw_spec_dir
            .or(raw_srcdir)
            .unwrap_or_else(|| "spec".to_string());
        let spec_dir = resolve_against_root(root, &spec_dir_rel);

        // builder
        let builder = match raw_builder.as_deref() {
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
        let issue_backend = match raw_issue_backend.as_deref() {
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
        let issue_doc = raw_issue_doc
            .as_deref()
            .map(|p| resolve_against_root(root, p));

        // roles — move out of the Option directly (no extra clone per field)
        let roles: HashMap<String, String> = raw_roles.unwrap_or_default();

        // Fail-on-drift: every directive value in roles must appear in [[needs.types]]
        // O(roles) thanks to the HashSet built above.
        if !roles.is_empty() {
            let mut drift: Vec<(String, String)> = roles
                .iter()
                .filter(|(_, directive)| !declared_directives.contains(*directive))
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

        // Destructure gate once so all fields can be moved independently.
        let (gate_needs_json, gate_sphinx_command, gate_exempt_statuses) = match raw_gate {
            Some(g) => (g.needs_json, g.sphinx_command, g.exempt_statuses),
            None => (None, None, None),
        };

        // gate.needs_json: from table or default <spec_dir>/_build/needs/needs.json
        let needs_json = gate_needs_json
            .as_deref()
            .map(|p| resolve_against_root(root, p))
            .unwrap_or_else(|| spec_dir.join("_build/needs/needs.json"));

        // gate.sphinx_command: from table or default
        let sphinx_command = match gate_sphinx_command {
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
            Some(cmd) => {
                if let Some(pos) = cmd.iter().position(|s| s.is_empty()) {
                    return Err(Error::Config {
                        message: format!(
                            "gate.sphinx_command element at index {pos} is an empty string; \
                             all sphinx_command elements must be non-empty"
                        ),
                    });
                }
                cmd
            }
        };

        // gate.exempt_statuses: default to ["done", "wontfix"] when absent.
        let exempt_statuses =
            gate_exempt_statuses.unwrap_or_else(|| vec!["done".to_string(), "wontfix".to_string()]);

        // lint table
        let lint = raw_lint
            .map(|raw| validate_lint(raw, &declared_directives))
            .transpose()?;

        // dedup table
        let dedup_threshold = match raw_dedup.and_then(|d| d.threshold) {
            Some(t) => {
                validate_threshold(t, "dedup.threshold")?;
                t
            }
            None => crate::retrieval::DEFAULT_THRESHOLD,
        };

        // rubrics tables
        let rubrics = validate_rubrics(raw_rubrics)?;

        // verdicts table
        let verdicts = raw_verdicts
            .map(|raw| validate_verdicts(raw, &declared_directives, &rubrics))
            .transpose()?;

        Ok(Config {
            spec_dir,
            builder,
            issue_backend,
            issue_doc,
            roles,
            needs_json,
            sphinx_command,
            exempt_statuses,
            lint,
            dedup_threshold,
            rubrics,
            verdicts,
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
    lint: Option<RawLintConfig>,
    dedup: Option<RawDedup>,
    rubrics: Option<HashMap<String, RawRubric>>,
    verdicts: Option<RawVerdicts>,
}

/// Raw `[tool.patdhlk-skills.rubrics.<name>]` — unknown keys ignored.
#[derive(Deserialize)]
struct RawRubric {
    axes: Option<Vec<String>>,
}

/// Raw `[tool.patdhlk-skills.verdicts]` — unknown keys ignored.
#[derive(Deserialize, Default)]
struct RawVerdicts {
    require: Option<HashMap<String, String>>,
    statuses: Option<Vec<String>>,
}

/// Raw `[tool.patdhlk-skills.dedup]` — threshold optional; unknown keys ignored.
#[derive(Deserialize, Default)]
struct RawDedup {
    threshold: Option<f64>,
}

#[derive(Deserialize)]
struct RawGate {
    needs_json: Option<String>,
    sphinx_command: Option<Vec<String>>,
    /// Statuses exempt from all corpus checks; absent → default `["done", "wontfix"]`.
    exempt_statuses: Option<Vec<String>>,
}

/// Raw `[tool.patdhlk-skills.lint]` — all rule keys optional; unknown keys ignored.
#[derive(Deserialize, Default)]
struct RawLintConfig {
    /// `required_sections`: map directive → list of required section lead-ins.
    required_sections: Option<HashMap<String, Vec<String>>>,
    /// `nontrivial_body`: map directive → minimum body length (integer chars).
    nontrivial_body: Option<HashMap<String, i64>>,
    /// `max_body_length`: map directive → maximum body length (integer chars).
    max_body_length: Option<HashMap<String, i64>>,
    /// `weasel_words`: `{ words = [...], directives = [...] }`.
    weasel_words: Option<RawWordListRule>,
    /// `unenumerated_quantifiers`: `{ quantifiers = [...], directives = [...] }`.
    unenumerated_quantifiers: Option<RawQuantifierRule>,
}

/// Shared raw type for word-list + directive-list rules.
#[derive(Deserialize)]
struct RawWordListRule {
    words: Vec<String>,
    directives: Vec<String>,
}

/// Shared raw type for quantifier-list + directive-list rules.
#[derive(Deserialize)]
struct RawQuantifierRule {
    quantifiers: Vec<String>,
    directives: Vec<String>,
}

// ---------------------------------------------------------------------------
// Lint validation
// ---------------------------------------------------------------------------

/// Validate a raw lint table into a [`LintConfig`].
///
/// All directive names referenced in any rule are checked against
/// `declared_directives`; any undeclared name is a [`Error::Config`] (exit 2).
fn validate_lint(
    raw: RawLintConfig,
    declared_directives: &HashSet<String>,
) -> Result<LintConfig, Error> {
    // ---- required_sections ------------------------------------------------
    let required_sections = raw
        .required_sections
        .map(|map| validate_required_sections(map, declared_directives))
        .transpose()?;

    // ---- nontrivial_body + max_body_length ---------------------------------
    // Merge the two per-directive maps into a single `body_length` map.
    let body_length = validate_body_length(
        raw.nontrivial_body,
        raw.max_body_length,
        declared_directives,
    )?;

    // ---- weasel_words ------------------------------------------------------
    let weasel_words = raw
        .weasel_words
        .map(|r| validate_word_list_rule(r, declared_directives, "weasel_words"))
        .transpose()?;

    // ---- unenumerated_quantifiers ------------------------------------------
    let unenumerated_quantifiers = raw
        .unenumerated_quantifiers
        .map(|r| validate_quantifier_rule(r, declared_directives))
        .transpose()?;

    Ok(LintConfig {
        required_sections,
        body_length,
        weasel_words,
        unenumerated_quantifiers,
    })
}

/// Validate `required_sections` map entries.
///
/// Every directive key must be in `declared_directives`; every inner `Vec`
/// must be non-empty (empty section list makes the key meaningless).
fn validate_required_sections(
    map: HashMap<String, Vec<String>>,
    declared_directives: &HashSet<String>,
) -> Result<HashMap<String, Vec<String>>, Error> {
    // An empty outer map is config noise — fail-closed.
    if map.is_empty() {
        return Err(Error::Config {
            message: "lint.required_sections must not be an empty table; \
                      remove the key or add at least one directive entry"
                .to_string(),
        });
    }

    let mut drift: Vec<String> = map
        .keys()
        .filter(|d| !declared_directives.contains(*d))
        .cloned()
        .collect();
    if !drift.is_empty() {
        drift.sort();
        return Err(Error::Config {
            message: format!(
                "lint.required_sections references undeclared [[needs.types]] \
                 directives: {}",
                drift.join(", ")
            ),
        });
    }
    // Inner vecs may not be empty (an empty section list is nonsensical).
    for (directive, sections) in &map {
        if sections.is_empty() {
            return Err(Error::Config {
                message: format!(
                    "lint.required_sections[{directive:?}]: section list must not be empty"
                ),
            });
        }
        // Individual section names may not be empty strings.
        if let Some(pos) = sections.iter().position(|s| s.is_empty()) {
            return Err(Error::Config {
                message: format!(
                    "lint.required_sections[{directive:?}]: section name at index {pos} \
                     is an empty string; all section names must be non-empty"
                ),
            });
        }
    }
    Ok(map)
}

/// Merge `nontrivial_body` (min) and `max_body_length` (max) into a single
/// per-directive [`LintBodyLength`] map, validating along the way.
///
/// Rules:
/// - Each length value must be > 0.
/// - Each directive key must be in `declared_directives`.
/// - A directive may appear in one or both maps.
/// - The result is `None` when both input maps are absent.
fn validate_body_length(
    min_map: Option<HashMap<String, i64>>,
    max_map: Option<HashMap<String, i64>>,
    declared_directives: &HashSet<String>,
) -> Result<Option<HashMap<String, LintBodyLength>>, Error> {
    if min_map.is_none() && max_map.is_none() {
        return Ok(None);
    }

    // Reject empty tables for each supplied key — an empty rule table is config noise.
    if let Some(ref m) = min_map
        && m.is_empty()
    {
        return Err(Error::Config {
            message: "lint.nontrivial_body must not be an empty table; \
                      remove the key or add at least one directive entry"
                .to_string(),
        });
    }
    if let Some(ref m) = max_map
        && m.is_empty()
    {
        return Err(Error::Config {
            message: "lint.max_body_length must not be an empty table; \
                      remove the key or add at least one directive entry"
                .to_string(),
        });
    }

    // Collect all directive keys from both maps for drift check.
    let mut all_directives: HashSet<String> = HashSet::new();
    if let Some(ref m) = min_map {
        all_directives.extend(m.keys().cloned());
    }
    if let Some(ref m) = max_map {
        all_directives.extend(m.keys().cloned());
    }

    let mut drift: Vec<String> = all_directives
        .iter()
        .filter(|d| !declared_directives.contains(*d))
        .cloned()
        .collect();
    if !drift.is_empty() {
        drift.sort();
        return Err(Error::Config {
            message: format!(
                "lint body-length rules reference undeclared [[needs.types]] \
                 directives: {}",
                drift.join(", ")
            ),
        });
    }

    // Validate individual values.
    if let Some(ref m) = min_map {
        for (directive, &val) in m {
            if val <= 0 {
                return Err(Error::Config {
                    message: format!(
                        "lint.nontrivial_body[{directive:?}]: minimum body length \
                         must be > 0, got {val}"
                    ),
                });
            }
        }
    }
    if let Some(ref m) = max_map {
        for (directive, &val) in m {
            if val <= 0 {
                return Err(Error::Config {
                    message: format!(
                        "lint.max_body_length[{directive:?}]: maximum body length \
                         must be > 0, got {val}"
                    ),
                });
            }
        }
    }

    // Build the merged map.
    let mut merged: HashMap<String, LintBodyLength> = HashMap::new();
    for directive in all_directives {
        let min = min_map
            .as_ref()
            .and_then(|m| m.get(&directive))
            .map(|&v| v as usize);
        let max = max_map
            .as_ref()
            .and_then(|m| m.get(&directive))
            .map(|&v| v as usize);
        merged.insert(directive, LintBodyLength { min, max });
    }

    Ok(Some(merged))
}

/// Validate a `weasel_words = { words = [...], directives = [...] }` rule.
///
/// Both lists must be non-empty and contain no empty strings.  All directive
/// names must appear in `declared_directives`.
fn validate_word_list_rule(
    raw: RawWordListRule,
    declared_directives: &HashSet<String>,
    key: &str,
) -> Result<LintWeaselWords, Error> {
    // words list
    if raw.words.is_empty() {
        return Err(Error::Config {
            message: format!("lint.{key}.words must not be empty"),
        });
    }
    if let Some(pos) = raw.words.iter().position(|w| w.is_empty()) {
        return Err(Error::Config {
            message: format!(
                "lint.{key}.words[{pos}] is an empty string; all words must be non-empty"
            ),
        });
    }
    // directives list
    if raw.directives.is_empty() {
        return Err(Error::Config {
            message: format!("lint.{key}.directives must not be empty"),
        });
    }
    if let Some(pos) = raw.directives.iter().position(|d| d.is_empty()) {
        return Err(Error::Config {
            message: format!(
                "lint.{key}.directives[{pos}] is an empty string; all directives must be non-empty"
            ),
        });
    }
    // drift check
    let mut drift: Vec<String> = raw
        .directives
        .iter()
        .filter(|d| !declared_directives.contains(*d))
        .cloned()
        .collect();
    if !drift.is_empty() {
        drift.sort();
        return Err(Error::Config {
            message: format!(
                "lint.{key}.directives references undeclared [[needs.types]] \
                 directives: {}",
                drift.join(", ")
            ),
        });
    }
    Ok(LintWeaselWords {
        words: raw.words,
        directives: raw.directives,
    })
}

/// Validate an `unenumerated_quantifiers = { quantifiers = [...], directives = [...] }` rule.
///
/// Both lists must be non-empty and contain no empty strings.  All directive
/// names must appear in `declared_directives`.
fn validate_quantifier_rule(
    raw: RawQuantifierRule,
    declared_directives: &HashSet<String>,
) -> Result<LintUnenumeratedQuantifiers, Error> {
    // quantifiers list
    if raw.quantifiers.is_empty() {
        return Err(Error::Config {
            message: "lint.unenumerated_quantifiers.quantifiers must not be empty".to_string(),
        });
    }
    if let Some(pos) = raw.quantifiers.iter().position(|q| q.is_empty()) {
        return Err(Error::Config {
            message: format!(
                "lint.unenumerated_quantifiers.quantifiers[{pos}] is an empty string; \
                 all quantifiers must be non-empty"
            ),
        });
    }
    // directives list
    if raw.directives.is_empty() {
        return Err(Error::Config {
            message: "lint.unenumerated_quantifiers.directives must not be empty".to_string(),
        });
    }
    if let Some(pos) = raw.directives.iter().position(|d| d.is_empty()) {
        return Err(Error::Config {
            message: format!(
                "lint.unenumerated_quantifiers.directives[{pos}] is an empty string; \
                 all directives must be non-empty"
            ),
        });
    }
    // drift check
    let mut drift: Vec<String> = raw
        .directives
        .iter()
        .filter(|d| !declared_directives.contains(*d))
        .cloned()
        .collect();
    if !drift.is_empty() {
        drift.sort();
        return Err(Error::Config {
            message: format!(
                "lint.unenumerated_quantifiers.directives references undeclared \
                 [[needs.types]] directives: {}",
                drift.join(", ")
            ),
        });
    }
    Ok(LintUnenumeratedQuantifiers {
        quantifiers: raw.quantifiers,
        directives: raw.directives,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate a dedup similarity threshold: finite, > 0, ≤ 1.
///
/// `key` names the source in the error message (`"dedup.threshold"` for the
/// config table, `"--threshold"` for the CLI flag).
pub(crate) fn validate_threshold(value: f64, key: &str) -> Result<(), Error> {
    if !value.is_finite() || value <= 0.0 || value > 1.0 {
        return Err(Error::Config {
            message: format!("{key} must be a number in (0, 1], got {value}"),
        });
    }
    Ok(())
}

/// Validate `[tool.patdhlk-skills.rubrics.*]`: every rubric needs a
/// non-empty `axes` list of non-empty, unique strings.
fn validate_rubrics(
    raw: Option<HashMap<String, RawRubric>>,
) -> Result<HashMap<String, Vec<String>>, Error> {
    let Some(raw) = raw else {
        return Ok(HashMap::new());
    };
    let mut rubrics: HashMap<String, Vec<String>> = HashMap::new();
    for (name, rubric) in raw {
        let axes = rubric.axes.unwrap_or_default();
        if axes.is_empty() {
            return Err(Error::Config {
                message: format!("rubrics.{name}: axes must be a non-empty list"),
            });
        }
        if let Some(pos) = axes.iter().position(|a| a.is_empty()) {
            return Err(Error::Config {
                message: format!("rubrics.{name}: axis at index {pos} is an empty string"),
            });
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for axis in &axes {
            if !seen.insert(axis.as_str()) {
                return Err(Error::Config {
                    message: format!("rubrics.{name}: duplicate axis {axis:?}"),
                });
            }
        }
        rubrics.insert(name, axes);
    }
    Ok(rubrics)
}

/// Validate `[tool.patdhlk-skills.verdicts]` against declared directives and
/// declared rubrics (ADR_0016: a require entry naming an undeclared rubric is
/// a config hard error).
fn validate_verdicts(
    raw: RawVerdicts,
    declared_directives: &HashSet<String>,
    rubrics: &HashMap<String, Vec<String>>,
) -> Result<VerdictsConfig, Error> {
    let require = raw.require.unwrap_or_default();
    if require.is_empty() {
        return Err(Error::Config {
            message: "verdicts table requires a non-empty `require` map \
                      ({ <type> = \"<rubric>\" })"
                .to_string(),
        });
    }
    let mut entries: Vec<(&String, &String)> = require.iter().collect();
    entries.sort();
    for (need_type, rubric) in entries {
        if !declared_directives.contains(need_type) {
            return Err(Error::Config {
                message: format!(
                    "verdicts.require references undeclared [[needs.types]] \
                     directive {need_type:?}"
                ),
            });
        }
        if !rubrics.contains_key(rubric) {
            return Err(Error::Config {
                message: format!(
                    "verdicts.require[{need_type:?}] names undeclared rubric \
                     {rubric:?}; declare [tool.patdhlk-skills.rubrics.{rubric}]"
                ),
            });
        }
    }
    if let Some(ref statuses) = raw.statuses {
        if statuses.is_empty() {
            return Err(Error::Config {
                message: "verdicts.statuses must not be an empty list; \
                          remove the key to mean all non-exempt statuses"
                    .to_string(),
            });
        }
        if let Some(pos) = statuses.iter().position(|s| s.is_empty()) {
            return Err(Error::Config {
                message: format!("verdicts.statuses element at index {pos} is an empty string"),
            });
        }
    }
    Ok(VerdictsConfig {
        require,
        statuses: raw.statuses,
    })
}

/// Join `root` and `rel` into an absolute, **lexically** normalised path.
///
/// Unlike [`project::absolutize`] this does *not* call `fs::canonicalize`;
/// the path need not exist yet. `..` and `.` components are folded away by
/// walking the component list: `.` is skipped, `..` pops the last segment.
fn resolve_against_root(root: &Path, rel: &str) -> PathBuf {
    // Start from the already-absolute root (or use `rel` directly when absolute).
    let base = {
        let p = Path::new(rel);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    };

    // Lexically normalise: fold `.` and `..` without hitting the filesystem.
    let mut out = PathBuf::new();
    for component in base.components() {
        match component {
            Component::CurDir => {} // skip `.`
            Component::ParentDir => {
                out.pop();
            } // fold `..`
            c => out.push(c),
        }
    }
    out
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
    // Path normalization: resolve_against_root must not leave .. or . components
    // ------------------------------------------------------------------

    #[test]
    fn spec_dir_with_dotdot_is_normalized() {
        // spec_dir = "spec/../docs" should yield <root>/docs with no ".." components.
        let toml = r#"
[tool.patdhlk-skills]
spec_dir = "spec/../docs"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let rendered = cfg.spec_dir.display().to_string();
        assert!(
            !rendered.contains(".."),
            "spec_dir must not contain '..', got: {rendered}"
        );
        assert!(
            cfg.spec_dir.ends_with("docs"),
            "spec_dir should resolve to docs, got: {rendered}"
        );
    }

    #[test]
    fn spec_dir_with_single_dot_is_normalized() {
        // spec_dir = "spec/./sub" should yield <root>/spec/sub with no "." components.
        let toml = r#"
[tool.patdhlk-skills]
spec_dir = "spec/./sub"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let rendered = cfg.spec_dir.display().to_string();
        // The path must not contain "/./", and ends_with checks the suffix.
        assert!(
            !rendered.contains("/./"),
            "spec_dir must not contain '.', got: {rendered}"
        );
        assert!(
            cfg.spec_dir.ends_with("spec/sub"),
            "spec_dir should resolve to spec/sub, got: {rendered}"
        );
    }

    #[test]
    fn needs_json_with_dotdot_is_normalized() {
        let toml = r#"
[tool.patdhlk-skills.gate]
needs_json = "spec/../other/_build/needs.json"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let rendered = cfg.needs_json.display().to_string();
        assert!(
            !rendered.contains(".."),
            "needs_json must not contain '..', got: {rendered}"
        );
        assert!(
            cfg.needs_json.ends_with("other/_build/needs.json"),
            "got: {rendered}"
        );
    }

    #[test]
    fn issue_doc_with_dotdot_is_normalized() {
        let toml = r#"
[tool.patdhlk-skills]
issue_doc = "spec/../issues/index.rst"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let rendered = cfg.issue_doc.unwrap().display().to_string();
        assert!(
            !rendered.contains(".."),
            "issue_doc must not contain '..', got: {rendered}"
        );
        assert!(rendered.ends_with("issues/index.rst"), "got: {rendered}");
    }

    // ------------------------------------------------------------------
    // Empty-string elements in sphinx_command are rejected
    // ------------------------------------------------------------------

    #[test]
    fn sphinx_command_with_empty_string_element_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.gate]
sphinx_command = ["uv", "", "sphinx-build"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(
            matches!(err, Error::Config { .. }),
            "expected Config error, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("sphinx_command"),
            "error must mention sphinx_command, got: {msg}"
        );
    }

    #[test]
    fn sphinx_command_with_leading_empty_string_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.gate]
sphinx_command = [""]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
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

    // ------------------------------------------------------------------
    // exempt_statuses on gate
    // ------------------------------------------------------------------

    #[test]
    fn absent_gate_table_gives_default_exempt_statuses() {
        let toml = "[project]\nname = \"x\"";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(
            cfg.exempt_statuses,
            vec!["done".to_string(), "wontfix".to_string()],
            "default exempt_statuses must be [done, wontfix]"
        );
    }

    #[test]
    fn gate_table_without_exempt_statuses_gives_default() {
        let toml = r#"
[tool.patdhlk-skills.gate]
needs_json = "spec/_build/needs/needs.json"
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(
            cfg.exempt_statuses,
            vec!["done".to_string(), "wontfix".to_string()]
        );
    }

    #[test]
    fn explicit_exempt_statuses_overrides_default() {
        let toml = r#"
[tool.patdhlk-skills.gate]
exempt_statuses = ["archived", "superseded"]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(
            cfg.exempt_statuses,
            vec!["archived".to_string(), "superseded".to_string()]
        );
    }

    #[test]
    fn empty_exempt_statuses_is_legal() {
        // Explicit empty list = nothing is exempt.
        let toml = r#"
[tool.patdhlk-skills.gate]
exempt_statuses = []
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(
            cfg.exempt_statuses.is_empty(),
            "explicit empty exempt_statuses must be respected"
        );
    }

    // ------------------------------------------------------------------
    // lint table: absent → None
    // ------------------------------------------------------------------

    #[test]
    fn absent_lint_table_yields_none() {
        let toml = "[project]\nname = \"x\"";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.lint.is_none(), "absent lint table must yield None");
    }

    // ------------------------------------------------------------------
    // lint table: all rule keys absent → valid LintConfig with all None
    // ------------------------------------------------------------------

    #[test]
    fn empty_lint_table_is_valid_with_all_rules_none() {
        let toml = r#"
[tool.patdhlk-skills.lint]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let lint = cfg.lint.expect("lint table present → Some(LintConfig)");
        assert!(lint.required_sections.is_none());
        assert!(lint.body_length.is_none());
        assert!(lint.weasel_words.is_none());
        assert!(lint.unenumerated_quantifiers.is_none());
    }

    // ------------------------------------------------------------------
    // lint table: unknown keys are ignored (forward compat)
    // ------------------------------------------------------------------

    #[test]
    fn unknown_lint_keys_are_ignored() {
        let toml = r#"
[tool.patdhlk-skills.lint]
future_rule = "something"
another_future = 42
"#;
        let (_tmp, project) = make_project(toml);
        let result = Config::load(&project);
        assert!(
            result.is_ok(),
            "unknown lint keys should be ignored, got: {:?}",
            result
        );
    }

    // ------------------------------------------------------------------
    // lint table: full parse with all four rules + max_body_length
    // ------------------------------------------------------------------

    #[test]
    fn full_lint_table_parses_all_rules() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[[needs.types]]
directive = "arch-decision"

[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.required_sections]
arch-decision = ["Context", "Decision", "Consequences"]

[tool.patdhlk-skills.lint.nontrivial_body]
issue = 50

[tool.patdhlk-skills.lint.max_body_length]
issue = 2000

[tool.patdhlk-skills.lint.weasel_words]
words = ["significant", "appropriate", "robust"]
directives = ["req"]

[tool.patdhlk-skills.lint.unenumerated_quantifiers]
quantifiers = ["all", "every", "each"]
directives = ["req"]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let lint = cfg.lint.expect("lint must be Some");

        // required_sections
        let rs = lint
            .required_sections
            .expect("required_sections must be Some");
        let ad_sections = rs.get("arch-decision").expect("arch-decision key present");
        assert_eq!(
            ad_sections,
            &vec![
                "Context".to_string(),
                "Decision".to_string(),
                "Consequences".to_string()
            ]
        );

        // body_length: min only on issue
        let bl = lint.body_length.expect("body_length must be Some");
        let issue_bl = bl.get("issue").expect("issue key present");
        assert_eq!(issue_bl.min, Some(50));
        assert_eq!(issue_bl.max, Some(2000));

        // weasel_words
        let ww = lint.weasel_words.expect("weasel_words must be Some");
        assert_eq!(
            ww.words,
            vec![
                "significant".to_string(),
                "appropriate".to_string(),
                "robust".to_string()
            ]
        );
        assert_eq!(ww.directives, vec!["req".to_string()]);

        // unenumerated_quantifiers
        let uq = lint
            .unenumerated_quantifiers
            .expect("unenumerated_quantifiers must be Some");
        assert_eq!(
            uq.quantifiers,
            vec!["all".to_string(), "every".to_string(), "each".to_string()]
        );
        assert_eq!(uq.directives, vec!["req".to_string()]);
    }

    // ------------------------------------------------------------------
    // max_body_length only (no nontrivial_body) → min is None
    // ------------------------------------------------------------------

    #[test]
    fn max_body_length_only_sets_min_to_none() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.max_body_length]
req = 500
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        let lint = cfg.lint.expect("lint Some");
        let bl = lint.body_length.expect("body_length Some");
        let req_bl = bl.get("req").expect("req key");
        assert_eq!(req_bl.min, None);
        assert_eq!(req_bl.max, Some(500));
    }

    // ------------------------------------------------------------------
    // Validation errors: body-length bounds
    // ------------------------------------------------------------------

    #[test]
    fn nontrivial_body_zero_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.lint.nontrivial_body]
issue = 0
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("nontrivial_body"),
            "error must mention nontrivial_body, got: {msg}"
        );
        assert!(
            msg.contains("issue"),
            "error must name the directive, got: {msg}"
        );
        assert_eq!(err.kind(), "config");
    }

    #[test]
    fn nontrivial_body_negative_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.nontrivial_body]
req = -5
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("nontrivial_body") || msg.contains("minimum"));
    }

    #[test]
    fn max_body_length_zero_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.max_body_length]
req = 0
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("max_body_length"),
            "error must mention max_body_length, got: {msg}"
        );
    }

    // ------------------------------------------------------------------
    // Validation errors: empty word lists
    // ------------------------------------------------------------------

    #[test]
    fn weasel_words_empty_words_list_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.weasel_words]
words = []
directives = ["req"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("weasel_words"),
            "error must mention weasel_words, got: {msg}"
        );
        assert_eq!(err.kind(), "config");
    }

    #[test]
    fn weasel_words_empty_string_in_words_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.weasel_words]
words = ["good", ""]
directives = ["req"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("weasel_words"), "got: {msg}");
    }

    #[test]
    fn weasel_words_empty_directives_list_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.weasel_words]
words = ["significant"]
directives = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("weasel_words"), "got: {msg}");
    }

    #[test]
    fn weasel_words_empty_string_in_directives_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.weasel_words]
words = ["significant"]
directives = ["req", ""]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("weasel_words"), "got: {msg}");
    }

    // ------------------------------------------------------------------
    // Validation errors: empty quantifier lists
    // ------------------------------------------------------------------

    #[test]
    fn unenumerated_quantifiers_empty_quantifiers_list_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.unenumerated_quantifiers]
quantifiers = []
directives = ["req"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("unenumerated_quantifiers"), "got: {msg}");
        assert_eq!(err.kind(), "config");
    }

    #[test]
    fn unenumerated_quantifiers_empty_string_in_quantifiers_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.unenumerated_quantifiers]
quantifiers = ["all", ""]
directives = ["req"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("unenumerated_quantifiers"), "got: {msg}");
    }

    #[test]
    fn unenumerated_quantifiers_empty_directives_list_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.unenumerated_quantifiers]
quantifiers = ["all"]
directives = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("unenumerated_quantifiers"), "got: {msg}");
    }

    #[test]
    fn unenumerated_quantifiers_empty_string_in_directives_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "req"

[tool.patdhlk-skills.lint.unenumerated_quantifiers]
quantifiers = ["all"]
directives = ["req", ""]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("unenumerated_quantifiers"), "got: {msg}");
    }

    // ------------------------------------------------------------------
    // Fail-on-drift: undeclared directive in each lint rule position
    // ------------------------------------------------------------------

    #[test]
    fn required_sections_undeclared_directive_is_drift_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.lint.required_sections]
arch-decision = ["Context", "Decision"]
"#;
        // "arch-decision" not declared
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("arch-decision"),
            "error must name the undeclared directive, got: {msg}"
        );
        assert_eq!(err.kind(), "config");
    }

    #[test]
    fn nontrivial_body_undeclared_directive_is_drift_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.lint.nontrivial_body]
req = 100
"#;
        // "req" not declared
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("req"),
            "error must name the undeclared directive, got: {msg}"
        );
        assert_eq!(err.kind(), "config");
    }

    #[test]
    fn max_body_length_undeclared_directive_is_drift_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.lint.max_body_length]
feat = 500
"#;
        // "feat" not declared
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("feat"),
            "error must name the undeclared directive, got: {msg}"
        );
        assert_eq!(err.kind(), "config");
    }

    #[test]
    fn weasel_words_undeclared_directive_is_drift_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.lint.weasel_words]
words = ["significant"]
directives = ["req"]
"#;
        // "req" not declared
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("req"),
            "error must name the undeclared directive, got: {msg}"
        );
        assert_eq!(err.kind(), "config");
    }

    #[test]
    fn unenumerated_quantifiers_undeclared_directive_is_drift_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.lint.unenumerated_quantifiers]
quantifiers = ["all"]
directives = ["arch-decision"]
"#;
        // "arch-decision" not declared
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("arch-decision"),
            "error must name the undeclared directive, got: {msg}"
        );
        assert_eq!(err.kind(), "config");
    }

    // ------------------------------------------------------------------
    // Drift error is kind "config" (exit 2)
    // ------------------------------------------------------------------

    #[test]
    fn drift_error_is_kind_config() {
        // Use required_sections as the representative rule.
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.lint.required_sections]
undeclared-type = ["Section A"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert_eq!(err.kind(), "config");
    }

    // ------------------------------------------------------------------
    // required_sections: empty section list is a config error
    // ------------------------------------------------------------------

    #[test]
    fn required_sections_empty_string_in_section_name_is_config_error() {
        // An empty-string section name is meaningless and must be rejected.
        let toml = r#"
[[needs.types]]
directive = "arch-decision"

[tool.patdhlk-skills.lint.required_sections]
arch-decision = ["Context", "", "Consequences"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(
            matches!(err, Error::Config { .. }),
            "expected Config error, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("arch-decision"),
            "error must name the directive, got: {msg}"
        );
        assert!(
            msg.contains("required_sections"),
            "error must mention required_sections, got: {msg}"
        );
    }

    #[test]
    fn required_sections_empty_outer_map_is_config_error() {
        // An empty required_sections table is config noise; fail-closed.
        let toml = r#"
[tool.patdhlk-skills.lint]
required_sections = {}
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(
            matches!(err, Error::Config { .. }),
            "expected Config error, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("required_sections"),
            "error must name the key, got: {msg}"
        );
    }

    #[test]
    fn nontrivial_body_empty_outer_map_is_config_error() {
        // An empty nontrivial_body table is config noise; fail-closed.
        let toml = r#"
[tool.patdhlk-skills.lint]
nontrivial_body = {}
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(
            matches!(err, Error::Config { .. }),
            "expected Config error, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("nontrivial_body"),
            "error must name the key, got: {msg}"
        );
    }

    #[test]
    fn max_body_length_empty_outer_map_is_config_error() {
        // An empty max_body_length table is config noise; fail-closed.
        let toml = r#"
[tool.patdhlk-skills.lint]
max_body_length = {}
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(
            matches!(err, Error::Config { .. }),
            "expected Config error, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("max_body_length"),
            "error must name the key, got: {msg}"
        );
    }

    #[test]
    fn required_sections_empty_section_list_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "arch-decision"

[tool.patdhlk-skills.lint.required_sections]
arch-decision = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("arch-decision"),
            "error must name the directive, got: {msg}"
        );
        assert!(
            msg.contains("required_sections") || msg.contains("section list"),
            "error must mention required_sections or section list, got: {msg}"
        );
    }

    // ------------------------------------------------------------------
    // dedup table: threshold
    // ------------------------------------------------------------------

    #[test]
    fn absent_dedup_table_gives_default_threshold() {
        let toml = "[project]\nname = \"x\"";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.dedup_threshold, crate::retrieval::DEFAULT_THRESHOLD);
    }

    #[test]
    fn explicit_dedup_threshold_overrides_default() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 0.6
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.dedup_threshold, 0.6);
    }

    #[test]
    fn dedup_threshold_zero_is_config_error() {
        // 0 would gate on any positive-scoring hit — config noise, reject.
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 0.0
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("threshold"));
    }

    #[test]
    fn dedup_threshold_above_one_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 1.5
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("threshold") && msg.contains("1.5"),
            "got: {msg}"
        );
    }

    #[test]
    fn dedup_threshold_negative_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = -0.2
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
    }

    #[test]
    fn dedup_threshold_exactly_one_is_legal() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 1.0
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.dedup_threshold, 1.0);
    }

    #[test]
    fn unknown_dedup_keys_are_ignored() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 0.4
future_engine_key = "embed"
"#;
        let (_tmp, project) = make_project(toml);
        assert!(Config::load(&project).is_ok());
    }

    #[test]
    fn dedup_threshold_integer_one_is_legal() {
        // TOML integer 1 must coerce to 1.0 — users write `threshold = 1`.
        let toml = "[tool.patdhlk-skills.dedup]\nthreshold = 1\n";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.dedup_threshold, 1.0);
    }

    // ------------------------------------------------------------------
    // rubrics + verdicts tables (ISSUE_0014 / ADR_0016)
    // ------------------------------------------------------------------

    #[test]
    fn absent_rubrics_and_verdicts_yield_empty_and_none() {
        let (_tmp, project) = make_project("[project]\nname = \"x\"");
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.rubrics.is_empty());
        assert!(cfg.verdicts.is_none());
    }

    #[test]
    fn full_verdicts_config_parses() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category", "state", "actionability"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
statuses = ["ready-for-agent", "in-progress"]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(
            cfg.rubrics.get("triage").unwrap(),
            &vec![
                "category".to_string(),
                "state".to_string(),
                "actionability".to_string()
            ]
        );
        let v = cfg.verdicts.as_ref().unwrap();
        assert_eq!(v.require.get("issue").map(String::as_str), Some("triage"));
        assert_eq!(
            v.statuses.as_ref().unwrap(),
            &vec!["ready-for-agent".to_string(), "in-progress".to_string()]
        );
    }

    #[test]
    fn verdicts_statuses_absent_is_none() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.verdicts.unwrap().statuses.is_none());
    }

    #[test]
    fn verdicts_require_undeclared_rubric_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.verdicts]
require = { issue = "review" }
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("review"),
            "must name the undeclared rubric, got: {msg}"
        );
    }

    #[test]
    fn verdicts_require_undeclared_type_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { story = "triage" }
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("story"));
    }

    #[test]
    fn verdicts_without_require_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.verdicts]
statuses = ["in-progress"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("require"));
    }

    #[test]
    fn rubric_with_empty_axes_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.rubrics.triage]
axes = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("triage"));
    }

    #[test]
    fn rubric_with_duplicate_axes_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.rubrics.triage]
axes = ["category", "category"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("category"));
    }

    #[test]
    fn rubric_with_empty_string_axis_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.rubrics.triage]
axes = ["category", ""]
"#;
        let (_tmp, project) = make_project(toml);
        assert!(matches!(
            Config::load(&project).unwrap_err(),
            Error::Config { .. }
        ));
    }

    #[test]
    fn verdicts_empty_statuses_list_is_config_error() {
        // An empty scope list would demand verdicts of nothing — config noise.
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
statuses = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("statuses"));
    }

    #[test]
    fn unknown_verdicts_keys_are_ignored() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
future_key = 7
"#;
        let (_tmp, project) = make_project(toml);
        assert!(Config::load(&project).is_ok());
    }
}
