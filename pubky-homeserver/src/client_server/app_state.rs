use crate::data_directory::user_quota::UserQuotaCache;
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
    pub(crate) events_service: EventsService,
    pub(crate) metrics: Metrics,
    /// Deploy-time default storage quota (MB), applied to new users during open signup.
    /// `None` = no limit.
    pub(crate) default_storage_quota_mb: Option<u64>,
    /// Shared cache for resolved per-user limits (used by rate limiter).
    pub(crate) user_quota_cache: UserQuotaCache,
}
