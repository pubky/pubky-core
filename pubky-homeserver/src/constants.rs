// The default limit of a list api if no `limit` query parameter is provided.
pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

// Maximum number of users allowed in a single /events-stream request.
// Calculation based on HTTP GET parameter size limits:
// - Conservative 4KB URL limit: ~3896 bytes available for user params
// - Per-user cost: &user=<52-char-pubkey>:<cursor> ≈ 74 bytes
//   - cursor (i64 as string): ~15 bytes average (max 20 for i64::MAX)
// - Max users at 4KB: 3896 / 74 ≈ 52 users
// - Set to 50 for clean limit with safety margin for longer cursors
pub const MAX_EVENT_STREAM_USERS: usize = 50;
