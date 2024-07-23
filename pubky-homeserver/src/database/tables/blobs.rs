use std::{borrow::Cow, time::SystemTime};

use heed::{
    types::{Bytes, Str},
    BoxedError, BytesDecode, BytesEncode, Database,
};

/// hash of the blob => bytes.
pub type BlobsTable = Database<Bytes, Bytes>;

pub const BLOBS_TABLE: &str = "blobs";
