use serde_json::{Map, Value, json};

/// Construct the shared finding envelope: `{"check", "severity", "need", "message"}`.
///
/// This is the single canonical constructor for findings across all verbs:
/// - `builder` uses it for step failures (need = `None`)
/// - `lint` uses it for body-lint violations (need = `Some(id)`)
/// - `checker` verdict checks (ISSUE_0014) will be the third consumer
///
/// `severity` is always `"error"` today; the parameter is kept explicit so
/// callers remain readable and future severity levels need no API change.
pub fn finding(check: &str, severity: &str, need: Option<&str>, message: &str) -> Value {
    json!({
        "check": check,
        "severity": severity,
        "need": need.map(|s| Value::String(s.to_owned())).unwrap_or(Value::Null),
        "message": message,
    })
}

/// A successful verb run.
///
/// Carries the verb's JSON payload as a type-enforced object ([`Map`], so a
/// non-object payload is unrepresentable) plus whether the corpus failed. The bin
/// maps a clean outcome to exit 0 and a failed one to exit 1; tool/config problems
/// never reach here — they surface as [`crate::Error`] (exit 2).
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    payload: Map<String, Value>,
    failed: bool,
}

impl Outcome {
    /// A clean run carrying the given payload object.
    pub fn clean(payload: Map<String, Value>) -> Self {
        Outcome {
            payload,
            failed: false,
        }
    }

    /// A failed run (corpus violations found) carrying the given payload object.
    pub fn failed(payload: Map<String, Value>) -> Self {
        Outcome {
            payload,
            failed: true,
        }
    }

    /// The verb's JSON payload object (merged into the stdout envelope by the bin).
    pub fn payload(&self) -> &Map<String, Value> {
        &self.payload
    }

    /// Consume the outcome, yielding its payload object.
    pub fn into_payload(self) -> Map<String, Value> {
        self.payload
    }

    /// Whether the corpus failed: `false` => exit 0, `true` => exit 1.
    pub fn is_failed(&self) -> bool {
        self.failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_outcome_is_not_failed() {
        let outcome = Outcome::clean(Map::new());
        assert!(!outcome.is_failed());
    }

    #[test]
    fn failed_outcome_carries_payload() {
        let mut payload = Map::new();
        payload.insert("findings".into(), Value::Array(vec![Value::from(1)]));
        let outcome = Outcome::failed(payload);
        assert!(outcome.is_failed());
        assert!(outcome.payload().contains_key("findings"));
    }
}
