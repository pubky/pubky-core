use crate::metrics_server::routes::metrics::Metrics;
use crate::persistence::files::events::EventsService;
use crate::persistence::files::FileService;
use crate::persistence::sql::SqlDb;
use crate::SignupMode;
use pubky_common::auth::AuthVerifier;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    /// The SQL database connection.
    pub(crate) sql_db: SqlDb,
    pub(crate) file_service: FileService,
    pub(crate) signup_mode: SignupMode,
    /// If `Some(bytes)` the quota is enforced, else unlimited.
    pub(crate) user_quota_bytes: Option<u64>,
    pub(crate) events_service: EventsService,
    pub(crate) metrics: Metrics,
}
