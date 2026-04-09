use axum::extract::FromRef;

use crate::client_server::auth::AuthState;
use crate::metrics_server::routes::metrics::Metrics;
use crate::persistence::files::events::EventsService;
use crate::persistence::files::FileService;
use crate::persistence::sql::SqlDb;
use crate::SignupMode;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    /// Auth sub-state (extracted via `FromRef` by auth handlers).
    pub(crate) auth_state: AuthState,
    /// The SQL database connection.
    pub(crate) sql_db: SqlDb,
    pub(crate) file_service: FileService,
    pub(crate) signup_mode: SignupMode,
    /// If `Some(bytes)` the quota is enforced, else unlimited.
    pub(crate) user_quota_bytes: Option<u64>,
    pub(crate) events_service: EventsService,
    pub(crate) metrics: Metrics,
}

impl FromRef<AppState> for AuthState {
    fn from_ref(state: &AppState) -> Self {
        state.auth_state.clone()
    }
}
