//! PAM authentication for the lock screen.

use zeroize::Zeroizing;

/// Default PAM service name.
const DEFAULT_PAM_SERVICE: &str = "i3more-lock";

/// Get the PAM service name, allowing override via environment variable
/// for testing (e.g. `I3MORE_LOCK_PAM_SERVICE=i3more-lock-test`).
pub fn pam_service() -> String {
    std::env::var("I3MORE_LOCK_PAM_SERVICE").unwrap_or_else(|_| DEFAULT_PAM_SERVICE.to_string())
}

/// Get the current username from environment.
pub fn get_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Authenticate the user via PAM.
///
/// Uses the PAM service from `pam_service()` (default `"i3more-lock"`,
/// overridable via `I3MORE_LOCK_PAM_SERVICE` env var).
///
/// Requires a matching `/etc/pam.d/<service>` file:
/// ```text
/// # /etc/pam.d/i3more-lock
/// auth    include   login
/// account include   login
/// ```
pub fn authenticate(username: &str, password: &Zeroizing<String>) -> Result<(), String> {
    use pam_client::conv_mock::Conversation;
    use pam_client::{Context, Flag};

    let service = pam_service();
    let conv = Conversation::with_credentials(username, password.as_str());
    let mut context =
        Context::new(&service, Some(username), conv).map_err(|e| format!("PAM init: {}", e))?;

    context
        .authenticate(Flag::NONE)
        .map_err(|e| format!("Authentication failed: {}", e))?;

    context
        .acct_mgmt(Flag::NONE)
        .map_err(|e| format!("Account check failed: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_username_from_user_env() {
        std::env::set_var("USER", "testuser");
        assert_eq!(get_username(), "testuser");
    }

    #[test]
    fn get_username_fallback_to_logname() {
        std::env::remove_var("USER");
        std::env::set_var("LOGNAME", "loguser");
        assert_eq!(get_username(), "loguser");
        // Restore USER to avoid affecting other tests
        std::env::set_var("USER", "testuser");
    }

    #[test]
    fn pam_service_default() {
        std::env::remove_var("I3MORE_LOCK_PAM_SERVICE");
        assert_eq!(pam_service(), "i3more-lock");
    }

    #[test]
    fn pam_service_override() {
        std::env::set_var("I3MORE_LOCK_PAM_SERVICE", "i3more-lock-test");
        assert_eq!(pam_service(), "i3more-lock-test");
        std::env::remove_var("I3MORE_LOCK_PAM_SERVICE");
    }
}
