use std::{borrow::Cow, time::SystemTime};

use heed::{
    types::{Bytes, Str},
    BoxedError, BytesDecode, BytesEncode, Database,
};

use pubky_common::crypto::Hash;

/// hash of the blob => bytes.
pub type BlobsTable = Database<Hash, Bytes>;

pub const BLOBS_TABLE: &str = "blobs";
