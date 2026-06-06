//! Lenient reader for sphinx-needs `needs.json` corpus files.
//!
//! [`NeedsCorpus::load`] reads a `needs.json` produced by any sphinx-needs
//! builder (ubc or sphinx-build) and normalises the two historically different
//! wire shapes (object-keyed and array) into a single in-memory representation.
//!
//! # Version selection
//!
//! The file's `current_version` key is used when it names an existing entry in
//! `versions`.  When `current_version` is absent or names a missing version the
//! loader falls back to the sole version when there is exactly one; otherwise it
//! returns [`Error::Tool`] naming the ambiguity.
//!
//! # Need fields
//!
//! `id`, `type`, and `title` are treated as required — a need missing any of
//! them yields [`Error::Tool`] (fail-closed: the corpus is schema-checked
//! upstream).  `status` is optional (absent or JSON null → `None`).  `content`
//! defaults to an empty string.  `tags` and `links` default to empty `Vec`s.
//!
//! All other fields are preserved verbatim in [`Need::extras`] for use by
//! downstream verbs (`kind`, `implements`, `blocked_by`, …).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::error::Error;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single sphinx-needs need, normalised from the corpus.
#[derive(Debug, Clone)]
pub struct Need {
    /// The need's unique identifier (e.g. `ISSUE_0001`).
    pub id: String,
    /// The directive type (e.g. `"issue"`, `"feat"`, `"arch-decision"`).
    /// Named `need_type` to avoid shadowing the keyword `type`.
    pub need_type: String,
    /// Human-readable title.
    pub title: String,
    /// Triage / workflow status.  `None` when absent or JSON null.
    pub status: Option<String>,
    /// Body content of the need directive.  Empty string when absent.
    pub content: String,
    /// Tags attached to the need.  Empty when absent.
    pub tags: Vec<String>,
    /// Generic link list (`links:` field).  Empty when absent.
    pub links: Vec<String>,
    /// All other fields from the raw JSON object (e.g. `kind`, `implements`,
    /// `blocked_by`, `github`, `__source__`).
    pub extras: Map<String, Value>,
}

/// The parsed corpus from a single `needs.json` file.
///
/// Iteration is deterministic: needs are stored sorted by [`Need::id`].
#[derive(Debug, Clone)]
pub struct NeedsCorpus {
    /// Needs sorted by id for deterministic iteration order.
    needs: BTreeMap<String, Need>,
}

impl NeedsCorpus {
    /// Load and parse a `needs.json` file.
    ///
    /// Returns [`Error::Tool`] for any I/O, JSON parse, shape, or required-field
    /// problem, with an actionable message naming the path or offending need.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let raw = std::fs::read_to_string(path).map_err(|e| Error::Tool {
            message: format!("cannot read {}: {e}", path.display()),
        })?;

        let value: Value = serde_json::from_str(&raw).map_err(|e| Error::Tool {
            message: format!("invalid JSON in {}: {e}", path.display()),
        })?;

        let root = value.as_object().ok_or_else(|| Error::Tool {
            message: format!(
                "{}: expected a JSON object at root, got {}",
                path.display(),
                value_kind(&value)
            ),
        })?;

        // --- version selection ---
        let versions_val = root.get("versions").ok_or_else(|| Error::Tool {
            message: format!(
                "{}: missing required top-level key \"versions\"",
                path.display()
            ),
        })?;

        let versions = versions_val.as_object().ok_or_else(|| Error::Tool {
            message: format!(
                "{}: \"versions\" must be a JSON object, got {}",
                path.display(),
                value_kind(versions_val)
            ),
        })?;

        if versions.is_empty() {
            return Err(Error::Tool {
                message: format!(
                    "{}: \"versions\" object is empty — no needs to load",
                    path.display()
                ),
            });
        }

        let current_version = root
            .get("current_version")
            .and_then(Value::as_str)
            .unwrap_or("");

        let version_entry = if !current_version.is_empty() && versions.contains_key(current_version)
        {
            &versions[current_version]
        } else if versions.len() == 1 {
            versions.values().next().expect("len == 1")
        } else {
            return Err(Error::Tool {
                message: format!(
                    "{}: ambiguous version — current_version is {:?} but available versions are [{}]; \
                     specify a unique version or set current_version",
                    path.display(),
                    current_version,
                    versions.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            });
        };

        let version_obj = version_entry.as_object().ok_or_else(|| Error::Tool {
            message: format!(
                "{}: selected version entry must be a JSON object, got {}",
                path.display(),
                value_kind(version_entry)
            ),
        })?;

        // --- needs extraction (object OR array) ---
        let needs_val = version_obj.get("needs").ok_or_else(|| Error::Tool {
            message: format!(
                "{}: selected version entry is missing the \"needs\" key",
                path.display()
            ),
        })?;

        let raw_needs: Vec<&Map<String, Value>> = match needs_val {
            Value::Object(map) => {
                // Object form: keyed by id.
                map.values()
                    .map(|v| {
                        v.as_object().ok_or_else(|| Error::Tool {
                            message: format!(
                                "{}: each entry in the needs object must be a JSON object, got {}",
                                path.display(),
                                value_kind(v)
                            ),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            Value::Array(arr) => {
                // Array form: older sphinx-needs.
                arr.iter()
                    .enumerate()
                    .map(|(i, v)| {
                        v.as_object().ok_or_else(|| Error::Tool {
                            message: format!(
                                "{}: needs[{i}] must be a JSON object, got {}",
                                path.display(),
                                value_kind(v)
                            ),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            other => {
                return Err(Error::Tool {
                    message: format!(
                        "{}: \"needs\" must be a JSON object or array, got {}",
                        path.display(),
                        value_kind(other)
                    ),
                });
            }
        };

        // --- parse individual needs ---
        let mut needs = BTreeMap::new();
        for (index, obj) in raw_needs.into_iter().enumerate() {
            let need = parse_need(path, index, obj)?;
            needs.insert(need.id.clone(), need);
        }

        Ok(NeedsCorpus { needs })
    }

    /// Look up a need by its id.  Returns `None` when not found.
    pub fn get(&self, id: &str) -> Option<&Need> {
        self.needs.get(id)
    }

    /// Iterate over all needs in deterministic (id-sorted) order.
    pub fn iter(&self) -> impl Iterator<Item = &Need> {
        self.needs.values()
    }

    /// Number of needs in the corpus.
    pub fn len(&self) -> usize {
        self.needs.len()
    }

    /// Returns `true` when the corpus has no needs.
    pub fn is_empty(&self) -> bool {
        self.needs.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Parse a single need object, enforcing required fields and extracting extras.
fn parse_need(path: &Path, index: usize, obj: &Map<String, Value>) -> Result<Need, Error> {
    // Helper: required string field.
    let require_str = |field: &str| -> Result<String, Error> {
        match obj.get(field) {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(other) => Err(Error::Tool {
                message: format!(
                    "{}: need at index {index} has field {:?} with non-string value {}",
                    path.display(),
                    field,
                    value_kind(other)
                ),
            }),
            None => {
                let id_hint = obj.get("id").and_then(Value::as_str).unwrap_or("<unknown>");
                Err(Error::Tool {
                    message: format!(
                        "{}: need {:?} (index {index}) is missing required field {:?}",
                        path.display(),
                        id_hint,
                        field
                    ),
                })
            }
        }
    };

    let id = require_str("id")?;
    let need_type = require_str("type")?;
    let title = require_str("title")?;

    // status: optional (None when absent or null)
    let status = match obj.get("status") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Null) | None => None,
        Some(other) => {
            return Err(Error::Tool {
                message: format!(
                    "{}: need {:?} has \"status\" with non-string, non-null value {}",
                    path.display(),
                    id,
                    value_kind(other)
                ),
            });
        }
    };

    // content: optional string, default ""
    let content = match obj.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => {
            return Err(Error::Tool {
                message: format!(
                    "{}: need {:?} has \"content\" with non-string value {}",
                    path.display(),
                    id,
                    value_kind(other)
                ),
            });
        }
    };

    // tags: optional array of strings, default []
    let tags = extract_string_vec(path, &id, "tags", obj)?;

    // links: optional array of strings, default []
    let links = extract_string_vec(path, &id, "links", obj)?;

    // extras: everything else that is not a known field
    const KNOWN: &[&str] = &["id", "type", "title", "status", "content", "tags", "links"];
    let extras: Map<String, Value> = obj
        .iter()
        .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    Ok(Need {
        id,
        need_type,
        title,
        status,
        content,
        tags,
        links,
        extras,
    })
}

/// Extract an optional `Vec<String>` from a JSON field.  Absent or null → empty vec.
fn extract_string_vec(
    path: &Path,
    id: &str,
    field: &str,
    obj: &Map<String, Value>,
) -> Result<Vec<String>, Error> {
    match obj.get(field) {
        Some(Value::Array(arr)) => arr
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v.as_str().map(str::to_owned).ok_or_else(|| Error::Tool {
                    message: format!(
                        "{}: need {:?} field {:?}[{i}] must be a string, got {}",
                        path.display(),
                        id,
                        field,
                        value_kind(v)
                    ),
                })
            })
            .collect(),
        Some(Value::Null) | None => Ok(Vec::new()),
        Some(other) => Err(Error::Tool {
            message: format!(
                "{}: need {:?} field {:?} must be an array or null, got {}",
                path.display(),
                id,
                field,
                value_kind(other)
            ),
        }),
    }
}

/// Return a human-readable name for the JSON value kind (for error messages).
fn value_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Helper: write inline JSON to a temp file and load it.
    fn load_inline(json: &str) -> Result<NeedsCorpus, Error> {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        NeedsCorpus::load(f.path())
    }

    // -----------------------------------------------------------------------
    // Object form
    // -----------------------------------------------------------------------

    #[test]
    fn object_form_loads_three_needs() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        assert_eq!(corpus.len(), 3);
    }

    #[test]
    fn object_form_ids_are_correct() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        assert!(corpus.get("FEAT_0001").is_some());
        assert!(corpus.get("ISSUE_0001").is_some());
        assert!(corpus.get("ISSUE_0002").is_some());
    }

    #[test]
    fn object_form_fields_parsed_correctly() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        let feat = corpus.get("FEAT_0001").unwrap();
        assert_eq!(feat.need_type, "feat");
        assert_eq!(feat.title, "Alpha feature");
        assert_eq!(feat.status.as_deref(), Some("ready-for-agent"));
        assert_eq!(feat.content, "Implements the alpha capability.");
        assert_eq!(feat.tags, vec!["alpha", "core"]);
        assert!(feat.links.is_empty());
    }

    #[test]
    fn object_form_extras_preserved() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        let feat = corpus.get("FEAT_0001").unwrap();
        // "kind" and "implements" should be in extras
        assert!(
            feat.extras.contains_key("kind"),
            "extras must contain 'kind'"
        );
        assert!(
            feat.extras.contains_key("implements"),
            "extras must contain 'implements'"
        );
        assert_eq!(
            feat.extras["kind"],
            Value::String("enhancement".to_string())
        );
        let implements = feat.extras["implements"].as_array().unwrap();
        assert_eq!(implements.len(), 1);
        assert_eq!(implements[0], Value::String("ISSUE_0002".to_string()));
    }

    #[test]
    fn object_form_null_status_is_none() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        let issue2 = corpus.get("ISSUE_0002").unwrap();
        assert!(issue2.status.is_none(), "null status should be None");
    }

    // -----------------------------------------------------------------------
    // Array form
    // -----------------------------------------------------------------------

    #[test]
    fn array_form_loads_same_three_needs() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-array-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        assert_eq!(corpus.len(), 3);
    }

    #[test]
    fn array_form_fields_match_object_form() {
        let obj_fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let arr_fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-array-form.json"
        ));
        let obj_corpus = NeedsCorpus::load(obj_fixture).unwrap();
        let arr_corpus = NeedsCorpus::load(arr_fixture).unwrap();

        for id in &["FEAT_0001", "ISSUE_0001", "ISSUE_0002"] {
            let obj_need = obj_corpus.get(id).unwrap();
            let arr_need = arr_corpus.get(id).unwrap();
            assert_eq!(obj_need.id, arr_need.id);
            assert_eq!(obj_need.need_type, arr_need.need_type);
            assert_eq!(obj_need.title, arr_need.title);
            assert_eq!(obj_need.status, arr_need.status);
            assert_eq!(obj_need.content, arr_need.content);
            assert_eq!(obj_need.tags, arr_need.tags);
            assert_eq!(obj_need.links, arr_need.links);
        }
    }

    // -----------------------------------------------------------------------
    // Deterministic iteration order
    // -----------------------------------------------------------------------

    #[test]
    fn iteration_is_sorted_by_id() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        let ids: Vec<&str> = corpus.iter().map(|n| n.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "iteration must be id-sorted");
    }

    // -----------------------------------------------------------------------
    // Version selection
    // -----------------------------------------------------------------------

    #[test]
    fn current_version_selects_named_version() {
        let json = r#"{
            "current_version": "v2",
            "project": "test",
            "versions": {
                "v1": { "needs": [] },
                "v2": {
                    "needs": {
                        "ISSUE_0001": {
                            "id": "ISSUE_0001",
                            "type": "issue",
                            "title": "From v2"
                        }
                    }
                }
            }
        }"#;
        let corpus = load_inline(json).unwrap();
        assert_eq!(corpus.len(), 1);
        assert_eq!(corpus.get("ISSUE_0001").unwrap().title, "From v2");
    }

    #[test]
    fn single_version_used_when_current_version_empty() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": {
                "": {
                    "needs": {
                        "ISSUE_0001": {
                            "id": "ISSUE_0001",
                            "type": "issue",
                            "title": "Only version"
                        }
                    }
                }
            }
        }"#;
        let corpus = load_inline(json).unwrap();
        assert_eq!(corpus.len(), 1);
    }

    #[test]
    fn current_version_missing_and_single_entry_falls_back() {
        // current_version names a version that doesn't exist → fall back to lone entry
        let json = r#"{
            "current_version": "ghost",
            "project": "test",
            "versions": {
                "only": {
                    "needs": {
                        "ISSUE_0001": {
                            "id": "ISSUE_0001",
                            "type": "issue",
                            "title": "Solo"
                        }
                    }
                }
            }
        }"#;
        let corpus = load_inline(json).unwrap();
        assert_eq!(corpus.len(), 1);
    }

    #[test]
    fn ambiguous_multi_version_without_current_is_tool_error() {
        let json = r#"{
            "current_version": "ghost",
            "project": "test",
            "versions": {
                "v1": { "needs": {} },
                "v2": { "needs": {} }
            }
        }"#;
        let err = load_inline(json).unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("ambiguous"),
            "must say 'ambiguous', got: {msg}"
        );
    }

    #[test]
    fn empty_versions_is_tool_error() {
        let json = r#"{"current_version": "", "project": "t", "versions": {}}"#;
        let err = load_inline(json).unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(msg.contains("empty"), "must say 'empty', got: {msg}");
    }

    // -----------------------------------------------------------------------
    // Required field enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn missing_id_is_tool_error() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": {
                "": {
                    "needs": [{ "type": "issue", "title": "No id" }]
                }
            }
        }"#;
        let err = load_inline(json).unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("\"id\""),
            "must name missing field 'id', got: {msg}"
        );
    }

    #[test]
    fn missing_type_is_tool_error() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": {
                "": {
                    "needs": [{ "id": "ISSUE_0001", "title": "No type" }]
                }
            }
        }"#;
        let err = load_inline(json).unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("\"type\""),
            "must name missing field 'type', got: {msg}"
        );
    }

    #[test]
    fn missing_title_is_tool_error_naming_the_id() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": {
                "": {
                    "needs": [{ "id": "ISSUE_0099", "type": "issue" }]
                }
            }
        }"#;
        let err = load_inline(json).unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("ISSUE_0099"),
            "must name the offending need id, got: {msg}"
        );
        assert!(
            msg.contains("\"title\""),
            "must name missing field 'title', got: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Optional field defaults
    // -----------------------------------------------------------------------

    #[test]
    fn absent_status_is_none() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": {
                "": {
                    "needs": [{ "id": "X_0001", "type": "feat", "title": "T" }]
                }
            }
        }"#;
        let corpus = load_inline(json).unwrap();
        let need = corpus.get("X_0001").unwrap();
        assert!(need.status.is_none());
    }

    #[test]
    fn absent_content_defaults_to_empty_string() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": {
                "": {
                    "needs": [{ "id": "X_0001", "type": "feat", "title": "T" }]
                }
            }
        }"#;
        let corpus = load_inline(json).unwrap();
        assert_eq!(corpus.get("X_0001").unwrap().content, "");
    }

    #[test]
    fn absent_tags_and_links_default_to_empty_vecs() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": {
                "": {
                    "needs": [{ "id": "X_0001", "type": "feat", "title": "T" }]
                }
            }
        }"#;
        let corpus = load_inline(json).unwrap();
        let need = corpus.get("X_0001").unwrap();
        assert!(need.tags.is_empty());
        assert!(need.links.is_empty());
    }

    // -----------------------------------------------------------------------
    // Malformed JSON
    // -----------------------------------------------------------------------

    #[test]
    fn invalid_json_is_tool_error() {
        let err = load_inline("{not valid json").unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("invalid JSON") || msg.contains("JSON"),
            "must mention JSON, got: {msg}"
        );
    }

    #[test]
    fn missing_versions_key_is_tool_error() {
        let err = load_inline(r#"{"current_version": "", "project": "x"}"#).unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("versions"),
            "must mention 'versions', got: {msg}"
        );
    }

    #[test]
    fn file_missing_is_tool_error_naming_path() {
        let err = NeedsCorpus::load(Path::new("/nonexistent/path/needs.json")).unwrap_err();
        assert!(matches!(err, Error::Tool { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent"),
            "must name the path, got: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // get() lookup
    // -----------------------------------------------------------------------

    #[test]
    fn get_returns_none_for_unknown_id() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/needs-object-form.json"
        ));
        let corpus = NeedsCorpus::load(fixture).unwrap();
        assert!(corpus.get("NONEXISTENT_9999").is_none());
    }

    // -----------------------------------------------------------------------
    // is_empty / len
    // -----------------------------------------------------------------------

    #[test]
    fn empty_needs_object_gives_empty_corpus() {
        let json = r#"{
            "current_version": "",
            "project": "test",
            "versions": { "": { "needs": {} } }
        }"#;
        let corpus = load_inline(json).unwrap();
        assert!(corpus.is_empty());
        assert_eq!(corpus.len(), 0);
    }
}
