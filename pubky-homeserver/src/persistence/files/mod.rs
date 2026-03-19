//! File storage and associated middleware.
//!
//! Blob I/O is handled by [`opendal`] (supporting filesystem, in-memory, and GCS
//! backends). Operations pass through a layered middleware stack (outermost first):
//!
//! 1. **[`events`]** — creates event records (PUT/DEL) after inner layers complete on close.
//! 2. **[`entry`]** — updates file metadata (blake3 hash, size, MIME type) in Postgres.
//! 3. **[`user_quota_layer`]** — enforces per-user storage quotas.
//! 4. **OpenDAL base** — physical storage I/O.
//!
//! [`file`] provides the high-level [`FileService`](file::file_service::FileService)
//! used by route handlers.

mod entry;
mod file;
mod opendal;

pub(crate) mod events;
mod user_quota_layer;
mod utils;

pub use file::file_io_error::{FileIoError, WriteStreamError};
pub(crate) use file::file_metadata::{FileMetadata, FileMetadataBuilder};
pub use file::file_service::FileService;
pub use file::file_stream_type::FileStream;
pub use opendal::opendal_service::OpendalService;
