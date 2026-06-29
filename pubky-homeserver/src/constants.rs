// The default limit of a list api if no `limit` query parameter is provided.
pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

/// Storage root for public, world-readable data (`/pub/...`).
pub const PUBLIC_ROOT: &str = "/pub/";

/// Storage root for private data (`/priv/...`), reads and writes require auth.
pub const PRIVATE_ROOT: &str = "/priv/";
