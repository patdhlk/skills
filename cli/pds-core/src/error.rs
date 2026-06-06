use thiserror::Error;

/// Failures classified by the pds exit contract.
///
/// `Error` is exit-code-2-only: it signals a tool or configuration problem that
/// prevented a verb from running. Corpus violations are *not* errors — they are a
/// successful verb run whose [`crate::Outcome`] carries non-empty findings and maps
/// to exit code 1.
#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {message}")]
    Config { message: String },

    #[error("tool error: {message}")]
    Tool { message: String },
}

impl Error {
    /// Process exit code per the pds contract: tool/config errors are always 2.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Config { .. } | Error::Tool { .. } => 2,
        }
    }

    /// The `error.kind` string emitted in the JSON envelope.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Config { .. } => "config",
            Error::Tool { .. } => "tool",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_contract() {
        assert_eq!(
            Error::Config {
                message: "x".into()
            }
            .exit_code(),
            2
        );
        assert_eq!(
            Error::Tool {
                message: "x".into()
            }
            .exit_code(),
            2
        );
    }

    #[test]
    fn kinds_match_json_envelope() {
        assert_eq!(
            Error::Config {
                message: "x".into()
            }
            .kind(),
            "config"
        );
        assert_eq!(
            Error::Tool {
                message: "x".into()
            }
            .kind(),
            "tool"
        );
    }
}
