//! The error every command returns to the UI. It carries codes the UI
//! translates, never secrets: no passphrase, device secret, session token or
//! quit code appears in any variant.

use clockin_sync::{ApiError, ErrorCode};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CmdError {
    /// The build has no server configuration.
    NotConfigured,
    /// This device isn't paired.
    NotPaired,
    /// The server couldn't be reached (offline, timeout, server error).
    Offline,
    /// The server refused the call (`bad_passphrase`, `locked`, `no_session`, …).
    #[serde(rename_all = "camelCase")]
    Rejected {
        code: String,
        retry_after_s: Option<u32>,
        details: Option<String>,
    },
    /// Input that the app's own checks refused (`field` names the input).
    #[serde(rename_all = "camelCase")]
    Invalid {
        field: &'static str,
        problem: String,
        /// Unknown passphrase words (0-based positions).
        #[serde(skip_serializing_if = "Option::is_none")]
        positions: Option<Vec<usize>>,
        /// A character not allowed in a name.
        #[serde(skip_serializing_if = "Option::is_none")]
        character: Option<char>,
    },
    /// Something went wrong on this computer.
    Internal { message: String },
}

impl CmdError {
    #[must_use]
    pub fn invalid(field: &'static str, problem: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            problem: problem.into(),
            positions: None,
            character: None,
        }
    }

    #[must_use]
    pub fn internal(message: impl ToString) -> Self {
        Self::Internal {
            message: message.to_string(),
        }
    }

    /// The server code, if it refused the call.
    #[cfg(test)]
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Rejected { code, .. } => Some(code),
            _ => None,
        }
    }
}

#[must_use]
pub fn code_name(code: &ErrorCode) -> String {
    match code {
        ErrorCode::BadSecret => "bad_secret",
        ErrorCode::Revoked => "revoked",
        ErrorCode::Locked => "locked",
        ErrorCode::BadPassphrase => "bad_passphrase",
        ErrorCode::NoSession => "no_session",
        ErrorCode::InvalidInput => "invalid_input",
        ErrorCode::VersionConflict => "version_conflict",
        ErrorCode::AlreadyInitialized => "already_initialized",
        ErrorCode::NotInitialized => "not_initialized",
        ErrorCode::Other(other) => other,
    }
    .to_owned()
}

impl From<ApiError> for CmdError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::Rejected(r) => Self::Rejected {
                code: code_name(&r.error),
                retry_after_s: r.retry_after_s,
                details: r.details,
            },
            e if e.is_unreachable() => Self::Offline,
            e => Self::internal(e),
        }
    }
}

impl From<clockin_sync::StoreError> for CmdError {
    fn from(e: clockin_sync::StoreError) -> Self {
        Self::internal(e)
    }
}

impl From<crate::secrets::SecretError> for CmdError {
    fn from(e: crate::secrets::SecretError) -> Self {
        Self::internal(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clockin_sync::Rejection;

    #[test]
    fn errors_serialise_for_the_ui() {
        let e: CmdError = ApiError::Rejected(Rejection {
            error: ErrorCode::Locked,
            retry_after_s: Some(60),
            details: None,
            config_version: None,
        })
        .into();
        assert_eq!(e.code(), Some("locked"));
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "rejected");
        assert_eq!(json["retryAfterS"], 60);

        let offline: CmdError = ApiError::Network("dns".into()).into();
        assert_eq!(offline, CmdError::Offline);
        assert_eq!(serde_json::to_value(&offline).unwrap()["kind"], "offline");

        let json = serde_json::to_value(CmdError::invalid("quit_code", "format")).unwrap();
        assert_eq!(json["field"], "quit_code");
        assert!(json.get("positions").is_none());
    }
}
