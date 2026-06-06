use thiserror::Error;

/// Failures classified by the pds exit contract.
///
/// `Config` and `Tool` both map to exit code 2; `Violations` is reserved for
/// exit code 1 (corpus violations) and gains structure in later tasks.
#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {message}")]
    Config { message: String },

    #[error("tool error: {message}")]
    Tool { message: String },

    #[error("{count} violation(s) found")]
    Violations { count: usize },
}

impl Error {
    /// Process exit code per the pds contract: 1 = violations, 2 = tool/config.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Violations { .. } => 1,
            Error::Config { .. } | Error::Tool { .. } => 2,
        }
    }

    /// The `error.kind` string emitted in the JSON envelope.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Config { .. } => "config",
            Error::Tool { .. } => "tool",
            Error::Violations { .. } => "violations",
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
        assert_eq!(Error::Violations { count: 3 }.exit_code(), 1);
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
