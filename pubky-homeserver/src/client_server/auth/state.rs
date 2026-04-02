//! Auth-specific sub-state for the auth module.

use crate::app_context::AppContext;
use crate::persistence::sql::SqlDb;
use crate::SignupMode;
use super::cookie::verifier::AuthVerifier;

use super::AuthService;

/// Auth-specific state. Auth route handlers extract this instead of the
/// global `AppState`, keeping the auth module fully self-contained.
#[derive(Clone, Debug)]
pub struct AuthState {
    pub(crate) auth_service: AuthService,
    pub(crate) sql_db: SqlDb,
    pub(crate) verifier: AuthVerifier,
    pub(crate) signup_mode: SignupMode,
}

impl AuthState {
    pub fn new(context: &AppContext) -> Self {
        Self {
            auth_service: AuthService::new(
                context.sql_db.clone(),
                context.keypair.clone(),
            ),
            sql_db: context.sql_db.clone(),
            verifier: AuthVerifier::default(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
        }
    }
}
