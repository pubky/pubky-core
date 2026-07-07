//! Event streaming endpoints.
//!
//! Provides two ways to consume file change events:
//! - `GET /events/` — Historical plain-text feed with cursor-based pagination.
//! - `GET /events-stream` — Server-Sent Events with a two-phase approach:
//!   first replays historical events from the database, then switches to
//!   real-time broadcast for live updates.

use axum::{
    body::Body,
    extract::{RawQuery, State},
    http::{header, HeaderMap, Response, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
};
use futures_util::stream::Stream;
use pubky_common::crypto::PublicKey;
use serde::Deserialize;
use std::{collections::HashMap, convert::Infallible, time::Instant};
use tower_cookies::Cookies;
use url::form_urlencoded;

use crate::{
    client_server::{
        auth::{has_read_permission, AuthSession},
        query_params::ListQueryParams,
        AppState,
    },
    constants::{PRIVATE_ROOT, PUBLIC_ROOT},
    metrics_server::routes::metrics::Metrics,
    persistence::{
        files::events::{
            EventCursor, EventEntity, EventsService, PathFilter, MAX_EVENT_STREAM_USERS,
        },
        sql::SqlDb,
    },
    shared::{webdav::WebDavPath, HttpError, HttpResult},
};

#[derive(Debug, thiserror::Error)]
pub enum EventStreamError {
    #[error("User not found")]
    UserNotFound,
    #[error("{0}")]
    InvalidParameter(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
}

impl From<EventStreamError> for HttpError {
    fn from(error: EventStreamError) -> Self {
        match error {
            EventStreamError::UserNotFound => HttpError::not_found(),
            EventStreamError::DatabaseError(e) => HttpError::from(e),
            _ => HttpError::bad_request(error.to_string()),
        }
    }
}

/// Query parameters for the event stream SSE endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawEventStreamQueryParams")]
pub struct EventStreamQueryParams {
    /// Maximum total events to send before closing connection.
    pub limit: Option<u16>,
    /// Return events in reverse chronological order
    /// **Cannot be combined with `live=true`** (returns 400 error).
    pub reverse: bool,
    /// Enable live streaming mode
    pub live: bool,
    /// One or more user public keys to filter events for.
    /// - Format: z-base-32 encoded public key (e.g., "o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo")
    /// - Single user: `?user=pubkey1`
    /// - Single user with cursor: `?user=pubkey1:cursor`
    /// - Multiple users: `?user=pubkey1&user=pubkey2:cursor2`
    pub user_cursors: Vec<(PublicKey, Option<String>)>,
    /// Repeatable path filters. Each value is a path WITHOUT the `pubky://`
    /// scheme or user pubkey, e.g. `/pub/files/`, `pub/files/`, `/priv/app/`..
    pub paths: Vec<WebDavPath>,
}

#[derive(Debug, Deserialize)]
struct RawEventStreamQueryParams {
    #[serde(default)]
    user: Vec<String>,
    limit: Option<u16>,
    #[serde(default)]
    reverse: bool,
    #[serde(default)]
    live: bool,
    #[serde(default)]
    paths: Vec<String>,
}

/// Parse query string manually to handle repeated `user` parameters.
/// URL query string format like: `user=pubkey1&user=pubkey2:cursor&limit=10&live=true`
fn parse_query_params(query: &str) -> Result<EventStreamQueryParams, EventStreamError> {
    let mut users = Vec::new();
    let mut limit = None;
    let mut reverse = false;
    let mut live = false;
    let mut paths = Vec::new();

    // Parse using form_urlencoded which handles URL decoding
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "user" => users.push(value.to_string()),
            "limit" => {
                limit = Some(value.parse::<u16>().map_err(|_| {
                    EventStreamError::InvalidParameter(format!("Invalid limit: {}", value))
                })?);
            }
            "reverse" => {
                reverse = value == "true" || value == "1";
            }
            "live" => {
                live = value == "true" || value == "1";
            }
            // `path` is repeatable; empty values are ignored.
            "path" => {
                if !value.is_empty() {
                    paths.push(value.to_string());
                }
            }
            _ => {} // Ignore unknown parameters
        }
    }

    let raw = RawEventStreamQueryParams {
        user: users,
        limit,
        reverse,
        live,
        paths,
    };

    raw.try_into()
}

impl TryFrom<RawEventStreamQueryParams> for EventStreamQueryParams {
    type Error = EventStreamError;

    fn try_from(raw: RawEventStreamQueryParams) -> Result<Self, Self::Error> {
        if raw.live && raw.reverse {
            return Err(EventStreamError::InvalidParameter(
                "Cannot use live mode with reverse ordering".to_string(),
            ));
        }

        // Parse user values into (pubkey, optional_cursor) pairs
        // Format: "pubkey" or "pubkey:cursor"
        let mut user_cursors = Vec::new();
        for value in raw.user {
            if value.is_empty() {
                continue;
            }

            let (pubkey_str, cursor_str) = if let Some((pubkey, cursor)) = value.split_once(':') {
                (pubkey, Some(cursor))
            } else {
                (value.as_str(), None)
            };

            if PublicKey::is_pubky_prefixed(pubkey_str) {
                return Err(EventStreamError::InvalidPublicKey(pubkey_str.to_string()));
            }
            let pubkey = PublicKey::try_from_z32(pubkey_str)
                .map_err(|_| EventStreamError::InvalidPublicKey(pubkey_str.to_string()))?;

            user_cursors.push((pubkey, cursor_str.map(|s| s.to_string())));
        }

        if user_cursors.is_empty() {
            return Err(EventStreamError::InvalidParameter(
                "user parameter is required".to_string(),
            ));
        }

        if user_cursors.len() > MAX_EVENT_STREAM_USERS {
            return Err(EventStreamError::InvalidParameter(format!(
                "Too many users. Maximum allowed: {}",
                MAX_EVENT_STREAM_USERS
            )));
        }

        // Parse each repeated `path` value. Empty values were already dropped
        // during query parsing.
        let mut paths = Vec::with_capacity(raw.paths.len());
        for p in raw.paths {
            let normalized_path = if p.starts_with('/') {
                p
            } else {
                format!("/{}", p)
            };

            let path = WebDavPath::new(&normalized_path).map_err(|_| {
                EventStreamError::InvalidParameter(format!("Invalid path: {}", normalized_path))
            })?;
            paths.push(path);
        }

        Ok(EventStreamQueryParams {
            limit: raw.limit,
            reverse: raw.reverse,
            live: raw.live,
            user_cursors,
            paths,
        })
    }
}

/// Format an event entity as SSE event data.
/// Returns the multiline data field content.
/// Each line will be prefixed with "data: " by the SSE library.
///
/// ## Format
/// ```text
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 42
/// data: content_hash: r0NJufX5oaagQE3qNtzJSZvLJcmtwRK3zJqTyuQfMmI= (only for PUT events, base64-encoded blake3 hash)
/// ```
fn formatted_event_path(entity: &EventEntity) -> String {
    // TODO: switch this formatter to use the shared `PubkyResource` type from `pubky-sdk`
    // once the homeserver crate depends on it directly, so we avoid ad-hoc string
    // reconstruction here.
    format!("pubky://{}{}", entity.user_pubkey.z32(), entity.path.path())
}

fn event_to_sse_data(entity: &EventEntity) -> String {
    let path = formatted_event_path(entity);
    let cursor_line = format!("cursor: {}", entity.cursor());

    let mut lines = vec![path, cursor_line];
    if let Some(hash) = entity.event_type.content_hash() {
        let hash_base64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, hash.as_bytes());
        lines.push(format!("content_hash: {}", hash_base64));
    }
    lines.join("\n")
}

/// Legacy text-based endpoint for fetching historical events.
///
/// This feed is public and unauthenticated: it returns events under
/// `/pub/...` exclusively and never exposes private (`/priv/...`) paths, even to
/// an authenticated caller.
///
/// ## Query Parameters
/// - `cursor` (optional): Starting cursor position. Default: "0" (beginning)
/// - `limit` (optional): Maximum number of events to return
///
/// ## Response Format
/// Plain text response with one line per event, followed by the next cursor:
/// ```text
/// PUT pubky://user_pubkey/pub/example.txt
/// DEL pubky://user_pubkey/pub/old.txt
/// PUT pubky://user_pubkey/pub/another.txt
/// cursor: 12345
/// ```
pub async fn feed(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> HttpResult<impl IntoResponse> {
    let cursor = match params.cursor {
        Some(cursor) => cursor,
        None => "0".to_string(),
    };

    let cursor = match state
        .events_service
        .parse_cursor(cursor.as_str(), &mut state.sql_db.pool().into())
        .await
    {
        Ok(cursor) => cursor,
        Err(_e) => return Err(HttpError::bad_request("Invalid cursor")),
    };

    let query_start = Instant::now();
    let events = state
        .events_service
        .get_public_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into())
        .await?;
    state
        .metrics
        .record_events_db_query(query_start.elapsed().as_millis());

    let mut result = events
        .iter()
        .map(|event| format!("{} {}", event.event_type, formatted_event_path(event)))
        .collect::<Vec<String>>();
    let next_cursor = events.last().map(|event| event.id.to_string());

    if let Some(next_cursor) = next_cursor {
        result.push(format!("cursor: {}", next_cursor));
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}

/// Server-Sent Events (SSE) endpoint for real-time event streaming.
///
/// Supports two modes:
/// - **Batch Mode** (`live=false` or omitted): Fetches historical events then closes connection
/// - **Streaming Mode** (`live=true`): Fetches historical events then streams new events in real-time
///
/// ## Slow Client Behavior
/// If a client cannot consume events fast enough in live mode, the broadcast channel will lag and the connection will be closed.
/// It is recommended that low memory clients poll this endpoint: Ie `live=true` with a low `limit`
///
/// ## Response Format
/// Each event is sent as an SSE message with the event type and multiline data:
/// ```text
/// event: PUT
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 42
/// data: content_hash: r0NJufX5oaagQE3qNtzJSZvLJcmtwRK3zJqTyuQfMmI=
/// ```
pub async fn feed_stream(
    State(state): State<AppState>,
    session: Option<AuthSession>,
    headers: HeaderMap,
    cookies: Cookies,
    raw_query: RawQuery,
) -> HttpResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let params =
        parse_query_params(raw_query.0.as_deref().unwrap_or("")).map_err(HttpError::from)?;

    // Grant sessions are set by middleware and take
    // priority. When no session was extracted AND no bearer was presented, fall
    // back to a same-tenant cookie for a single-user private subscription.
    let session = match session {
        Some(session) => Some(session),
        None if !has_bearer_auth(&headers) => {
            resolve_tenant_cookie_session(&state, &cookies, &params).await
        }
        None => None,
    };

    // Authorize the requested path filters before doing any work. Public paths
    // need no session; any `/priv/` path requires a session (grant or same-tenant
    // cookie) whose single user matches and that holds a covering read capability.
    let allowed_paths = authorized_paths(&params.paths, &params.user_cursors, session.as_ref())?;

    let mut user_cursor_map =
        resolve_user_cursors(&params.user_cursors, &state.events_service, &state.sql_db)
            .await
            .map_err(HttpError::from)?;

    let mut total_sent: usize = 0;
    let stream = async_stream::stream! {
         // Create guard to ensure cleanup on any exit path (increments on creation, decrements on drop)
        let _guard = ConnectionGuard::new(state.metrics.clone());

        // Subscribe to broadcast channel immediately to prevent race condition
        // Events that occur during Phase 1 will be buffered in the channel
        let mut rx = state.events_service.subscribe();

        // Phase 1: Batch Mode
        loop {
            // Drain any buffered events before querying as they'll be included in this or a future database query
            while rx.try_recv().is_ok() {}

            let current_user_cursors: Vec<(i32, Option<EventCursor>)> =
                user_cursor_map.iter().map(|(k, cursor)| (*k, *cursor)).collect();

            let query_start = Instant::now();
            let events = match state
                .events_service
                .get_by_user_cursors(
                    current_user_cursors,
                    params.reverse,
                    &allowed_paths,
                    &mut state.sql_db.pool().into(),
                )
                .await
            {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!("Database error while fetching events: {}", e);
                    break;
                }
            };
            state.metrics.record_event_stream_db_query(query_start.elapsed().as_millis());

            let event_count = events.len();

            // Stream each historical event
            for event in events {
                // Update the cursor for this specific user
                user_cursor_map.insert(event.user_id, Some(event.cursor()));

                yield Ok(Event::default()
                    .event(event.event_type.to_string())
                    .data(event_to_sse_data(&event)));

                total_sent += 1;

                // Close if we've reached limit
                if let Some(max) = params.limit {
                    if total_sent >= max as usize {
                        return;
                    }
                }
            }

            // If we got zero events, all users are caught up with history
            if event_count == 0 {
                // If not in live mode, close connection (batch mode)
                if !params.live {
                    return;
                }
                // Otherwise, transition to live mode
                break;
            }

            // If we got a partial batch (> 0 but less than read_batch_size), continue querying
            // Some users might still have more events even though this batch was partial
        }

        // Phase 2: Live mode
        if params.live {
            let user_ids: Vec<i32> = user_cursor_map.keys().copied().collect();
            let half_capacity = state.events_service.channel_capacity() / 2;

            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Check if receiver queue is at half capacity (early warning of slow clients)
                        if rx.len() >= half_capacity {
                            state.metrics.record_broadcast_half_full();
                        }
                        // Filter events based on user_ids, cursors, and path
                        if !should_include_live_event(&event, &user_ids, &user_cursor_map, &allowed_paths) {
                            continue;
                        }

                        // Update this user's cursor
                        user_cursor_map.insert(event.user_id, Some(event.cursor()));

                        yield Ok(Event::default()
                            .event(event.event_type.to_string())
                            .data(event_to_sse_data(&event)));

                        total_sent += 1;

                        // Close if we've reached limit
                        if let Some(max) = params.limit {
                            if total_sent >= max as usize {
                                return;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        state.metrics.record_broadcast_lagged();
                        tracing::warn!(
                            "Slow client detected: broadcast channel lagged by {} events. Closing connection.",
                            skipped
                        );
                        return;
                    }
                    Err(_) => break, // Channel closed
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Resolve user public keys to user IDs and parse their cursors.
/// Returns a map of user_id → optional cursor position.
async fn resolve_user_cursors(
    user_cursors: &[(PublicKey, Option<String>)],
    events_service: &EventsService,
    sql_db: &SqlDb,
) -> Result<HashMap<i32, Option<EventCursor>>, EventStreamError> {
    use crate::persistence::sql::user::UserRepository;

    let mut user_cursor_map: HashMap<i32, Option<EventCursor>> = HashMap::new();

    for (user_pubkey, cursor_str_opt) in user_cursors {
        let user_id = UserRepository::get_id(user_pubkey, &mut sql_db.pool().into())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => EventStreamError::UserNotFound,
                e => EventStreamError::DatabaseError(e),
            })?;

        let cursor = if let Some(cursor_str) = cursor_str_opt {
            Some(
                events_service
                    .parse_cursor(cursor_str, &mut sql_db.pool().into())
                    .await
                    .map_err(|_| {
                        EventStreamError::InvalidParameter(format!(
                            "Invalid cursor: {}",
                            cursor_str
                        ))
                    })?,
            )
        } else {
            None
        };

        user_cursor_map.insert(user_id, cursor);
    }

    Ok(user_cursor_map)
}

/// Whether the request presented a `Bearer` Authorization header (valid or not).
/// Mirrors the grant middleware's `strip_prefix("Bearer ")`, so a presented
/// bearer is never silently downgraded to cookie auth.
fn has_bearer_auth(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("Bearer "))
}

/// Resolve a same-tenant cookie session for a single-user private subscription.
///
/// The cookie middleware keys cookies by `pubky-host` (the homeserver here), so a
/// user's cookie never matches on this endpoint. A private subscription names
/// exactly one user, so we look the cookie up by that tenant instead. Returns
/// `None` unless a `/priv/` path is requested AND exactly one `user=` is named;
/// [`resolve_session_from_cookie`](crate::client_server::auth::AuthState) then
/// verifies the cookie's session belongs to that user, so this grants no
/// authority the caller didn't already hold.
async fn resolve_tenant_cookie_session(
    state: &AppState,
    cookies: &Cookies,
    params: &EventStreamQueryParams,
) -> Option<AuthSession> {
    let requests_private = params
        .paths
        .iter()
        .any(|path| path.as_str().starts_with(PRIVATE_ROOT));
    if !requests_private {
        return None;
    }
    let [(tenant, _)] = params.user_cursors.as_slice() else {
        return None;
    };
    let cookie_value = cookies.get(&tenant.z32()).map(|c| c.value().to_string());
    state
        .auth_state
        .cookie_auth_service
        .resolve_session_from_cookie(cookie_value, tenant)
        .await
}

/// Authorize the requested `path`s and return the allow-list to apply to both
/// the historical replay and the live phase.
///
/// - No requested path → implicit public-only (`/pub/`), no session needed.
/// - Public (`/pub/...`) paths need no capability.
/// - Any private (`/priv/...`) path requires a session (else 401), exactly one
///   `user=` equal to the session user (else 403), and a read capability
///   covering each private path (else 403).
///
/// The session may be grant- or (same-tenant) cookie-backed; both are accepted.
fn authorized_paths(
    paths: &[WebDavPath],
    user_cursors: &[(PublicKey, Option<String>)],
    session: Option<&AuthSession>,
) -> Result<Vec<PathFilter>, HttpError> {
    // No path requested: default to public-only.
    if paths.is_empty() {
        return Ok(vec![
            WebDavPath::new_unchecked(PUBLIC_ROOT.to_string()).into()
        ]);
    }

    // The tenant to authorize each path against. A private read is single-tenant,
    // so a tenant only exists when the subscription names exactly one user.
    let tenant = match user_cursors {
        [(pubkey, _)] => Some(pubkey),
        _ => None,
    };

    let mut allowed = Vec::with_capacity(paths.len());
    for path in paths {
        has_read_permission(session, tenant, path)?;
        allowed.push(path.clone().into());
    }
    Ok(allowed)
}

/// Filter events in live mode based on user IDs, cursors, and the authorized
/// paths.
fn should_include_live_event(
    event: &EventEntity,
    user_ids: &[i32],
    user_cursor_map: &HashMap<i32, Option<EventCursor>>,
    allowed_paths: &[PathFilter],
) -> bool {
    if !user_ids.contains(&event.user_id) {
        return false;
    }

    // Filter out events we already sent in Phase 1
    if let Some(Some(cursor)) = user_cursor_map.get(&event.user_id) {
        if event.cursor() <= *cursor {
            return false;
        }
    }

    // Apply the authorized filter set.
    let path = event.path.path().as_str();
    allowed_paths.iter().any(|filter| filter.matches(path))
}

/// Guard to ensure connection cleanup on any exit path
struct ConnectionGuard {
    metrics: Metrics,
    start: Instant,
}

impl ConnectionGuard {
    fn new(metrics: Metrics) -> Self {
        metrics.increment_active_connections();
        Self {
            metrics,
            start: Instant::now(),
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.metrics.decrement_active_connections();
        self.metrics
            .record_connection_closed(self.start.elapsed().as_millis());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_server::auth::grant::session::GrantSession;
    use pubky_common::auth::jws::GrantId;
    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_common::crypto::Keypair;

    fn pk() -> PublicKey {
        Keypair::random().public_key()
    }

    fn wd(s: &str) -> WebDavPath {
        WebDavPath::new(s).expect("valid test path")
    }

    fn pf(s: &str) -> PathFilter {
        wd(s).into()
    }

    fn grant_session(user_key: PublicKey, capabilities: Capabilities) -> AuthSession {
        AuthSession::Grant(GrantSession {
            user_key,
            capabilities,
            grant_id: GrantId::generate(),
            token_expires_at: 9999999999,
        })
    }

    fn cookie_session(user_key: PublicKey, capabilities: Capabilities) -> AuthSession {
        use crate::client_server::auth::cookie::persistence::{SessionEntity, SessionSecret};
        AuthSession::Cookie(SessionEntity {
            id: 1,
            secret: SessionSecret::random(),
            user_id: 1,
            user_pubkey: user_key,
            capabilities,
            created_at: sqlx::types::chrono::DateTime::from_timestamp(0, 0)
                .expect("valid timestamp")
                .naive_utc(),
        })
    }

    fn cursors(keys: &[&PublicKey]) -> Vec<(PublicKey, Option<String>)> {
        keys.iter().map(|k| ((*k).clone(), None)).collect()
    }

    fn reject_status(result: Result<Vec<PathFilter>, HttpError>) -> StatusCode {
        result
            .expect_err("expected the subscription to be rejected")
            .into_response()
            .status()
    }

    #[test]
    fn connection_guard_drops_on_early_return() {
        let metrics = Metrics::new().expect("Failed to create metrics");

        // Create guard and return early - guard should still decrement
        fn early_return_fn(metrics: Metrics) -> Result<(), &'static str> {
            let _guard = ConnectionGuard::new(metrics.clone());
            // Simulate early return (e.g., error condition)
            return Err("early exit");
            #[allow(unreachable_code)]
            {
                Ok(())
            }
        }

        let result = early_return_fn(metrics.clone());
        assert!(result.is_err(), "Should have returned early");

        // Verify guard cleaned up properly despite early return
        let output = metrics.render().expect("Failed to render metrics");
        assert!(
            output.contains("event_stream_active_connections") && output.contains("} 0"),
            "Should have 0 active connections after early return: {}",
            output
        );
        assert!(
            output.contains("event_stream_connection_duration_ms_count"),
            "Should have recorded connection duration: {}",
            output
        );
    }

    #[tokio::test]
    async fn connection_guard_concurrent() {
        let metrics = Metrics::new().expect("Failed to create metrics");

        // Create 5 concurrent guards using tokio::spawn
        let handles: Vec<_> = (0..5)
            .map(|i| {
                let metrics_clone = metrics.clone();
                tokio::spawn(async move {
                    let _guard = ConnectionGuard::new(metrics_clone);
                    // Simulate some work
                    tokio::time::sleep(tokio::time::Duration::from_millis(10 * i)).await;
                    // Guard will be dropped here
                })
            })
            .collect();

        // While tasks are running, check active connections
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let output = metrics.render().expect("Failed to render metrics");
        // We should have some active connections (implementation dependent on timing)
        assert!(
            output.contains("event_stream_active_connections"),
            "Should have active connections metric: {}",
            output
        );

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // All guards should be cleaned up
        let output = metrics.render().expect("Failed to render metrics");
        assert!(
            output.contains("event_stream_active_connections") && output.contains("} 0"),
            "Should have 0 active connections after all concurrent guards dropped: {}",
            output
        );
        assert!(
            output.contains("event_stream_connection_duration_ms_count") && output.contains("} 5"),
            "Should have recorded 5 connection durations: {}",
            output
        );
    }

    #[test]
    fn parse_repeated_paths_preserves_order_and_trailing_slash() {
        let q = format!(
            "user={}&path=/pub/&path=/priv/app/&path=/priv/file",
            pk().z32()
        );
        let params = parse_query_params(&q).unwrap();
        let strs: Vec<&str> = params.paths.iter().map(|p| p.as_str()).collect();
        assert_eq!(strs, vec!["/pub/", "/priv/app/", "/priv/file"]);
    }

    #[test]
    fn parse_ignores_empty_path_values() {
        let params =
            parse_query_params(&format!("user={}&path=&path=/pub/&path=", pk().z32())).unwrap();
        assert_eq!(
            params.paths.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            vec!["/pub/"]
        );
    }

    #[test]
    fn parse_requires_user() {
        let err = parse_query_params("path=/pub/").unwrap_err();
        assert_eq!(
            HttpError::from(err).into_response().status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn no_path_defaults_to_public_dir_filter() {
        let u = pk();
        let filters = authorized_paths(&[], &cursors(&[&u]), None).unwrap();
        assert_eq!(filters, vec![pf("/pub/")]);
    }

    #[test]
    fn anonymous_private_path_is_unauthorized() {
        let u = pk();
        let status = reject_status(authorized_paths(&[wd("/priv/app/")], &cursors(&[&u]), None));
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn cookie_session_authorizes_own_private_path() {
        // A same-tenant cookie session authorizes its own single-user private path.
        let owner = pk();
        let session = cookie_session(owner.clone(), Capabilities::from(vec![Capability::root()]));
        let filters =
            authorized_paths(&[wd("/priv/app/")], &cursors(&[&owner]), Some(&session)).unwrap();
        assert_eq!(filters, vec![pf("/priv/app/")]);
    }

    #[test]
    fn cookie_session_wrong_tenant_is_forbidden() {
        // A cookie session for A requesting B's private events is 403 (session user
        // ≠ tenant). (Resolution can't reach here with a mismatched cookie anyway.)
        let (a, b) = (pk(), pk());
        let session = cookie_session(a, Capabilities::from(vec![Capability::root()]));
        let status = reject_status(authorized_paths(
            &[wd("/priv/app/")],
            &cursors(&[&b]),
            Some(&session),
        ));
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn cookie_session_under_scoped_denies_sibling_private_path() {
        // A cookie session scoped to `/priv/app/` may not read a sibling scope.
        let owner = pk();
        let session = cookie_session(
            owner.clone(),
            Capabilities::from(vec![Capability::read("/priv/app/")]),
        );
        let status = reject_status(authorized_paths(
            &[wd("/priv/other/")],
            &cursors(&[&owner]),
            Some(&session),
        ));
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn cookie_session_allows_public_path() {
        // Public paths need no auth, so a cookie session doesn't change the outcome.
        let owner = pk();
        let session = cookie_session(owner.clone(), Capabilities::from(vec![Capability::root()]));
        let filters =
            authorized_paths(&[wd("/pub/")], &cursors(&[&owner]), Some(&session)).unwrap();
        assert_eq!(filters, vec![pf("/pub/")]);
    }

    #[test]
    fn private_path_with_multiple_users_is_forbidden() {
        let (a, b) = (pk(), pk());
        let session = grant_session(a.clone(), Capabilities::from(vec![Capability::root()]));
        let status = reject_status(authorized_paths(
            &[wd("/priv/app/")],
            &cursors(&[&a, &b]),
            Some(&session),
        ));
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn private_path_under_scoped_session_is_forbidden() {
        let owner = pk();
        let session = grant_session(
            owner.clone(),
            Capabilities::from(vec![Capability::read("/priv/app/")]),
        );
        // A sibling scope not covered by the read cap.
        let status = reject_status(authorized_paths(
            &[wd("/priv/other/")],
            &cursors(&[&owner]),
            Some(&session),
        ));
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn mixed_public_and_private_authorized_union() {
        let owner = pk();
        let session = grant_session(
            owner.clone(),
            Capabilities::from(vec![Capability::read("/priv/app/")]),
        );
        let filters = authorized_paths(
            &[wd("/pub/"), wd("/priv/app/")],
            &cursors(&[&owner]),
            Some(&session),
        )
        .unwrap();
        assert_eq!(filters, vec![pf("/pub/"), pf("/priv/app/")]);
    }

    #[test]
    fn public_paths_with_multiple_users_are_authorized() {
        let (a, b) = (pk(), pk());
        let filters = authorized_paths(&[wd("/pub/")], &cursors(&[&a, &b]), None).unwrap();
        assert_eq!(filters, vec![pf("/pub/")]);
    }
}
