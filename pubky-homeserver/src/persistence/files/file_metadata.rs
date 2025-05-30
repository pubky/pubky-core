use pubky_common::{crypto::{Hash, Hasher}, timestamp::Timestamp};


/// Metadata of a file.
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub hash: Hash,
    pub length: usize,
    pub modified_at: Timestamp,
}

/// Builder for FileMetadata.
/// This is used to build the FileMetadata from a stream of chunks.
#[derive(Default, Debug, Clone)]
pub struct FileMetadataBuilder {
    hasher: Hasher,
    length: usize,
}

impl FileMetadataBuilder {
    pub fn update(&mut self, chunk: &[u8]) {
        self.hasher.update(chunk);
        self.length += chunk.len();
    }

    pub fn finalize(self) -> FileMetadata {
        FileMetadata {
            hash: self.hasher.finalize(),
            length: self.length,
            modified_at: Timestamp::now(),
        }
    }
}


