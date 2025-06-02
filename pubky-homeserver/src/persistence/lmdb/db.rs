use super::tables::{Tables, TABLES_COUNT};
use focuson_cas::{FileSystemCAS, ContentAddressableStorage, StringStorage};
use heed::{Env, EnvOpenOptions};
use std::sync::Arc;
use std::{fs, path::PathBuf};
use anyhow::Result;

use super::migrations;

pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

#[derive(Debug, Clone)]
pub struct LmDB {
    pub(crate) env: Env,
    pub(crate) tables: Tables,
    pub(crate) max_chunk_size: usize,
    /// Content Addressable Storage for large blobs
    pub(crate) cas: FileSystemCAS,
    /// Threshold above which data is stored in CAS instead of LMDB
    pub(crate) cas_threshold: usize,
    // Only used for testing purposes to keep the testdir alive.
    #[allow(dead_code)]
    test_dir: Option<Arc<tempfile::TempDir>>,
}

/// Represents how blob data is stored
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlobStorage {
    /// Data stored directly in LMDB
    Inline(Vec<u8>),
    /// Data stored in CAS, with content hash as reference
    CasRef(String),
}

impl LmDB {
    /// # Safety
    /// DB uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub unsafe fn open(main_dir: PathBuf) -> anyhow::Result<Self> {
        Self::open_with_cas_threshold(main_dir, 64 * 1024) // Default 64KB threshold
    }

    /// # Safety
    /// Same as `open` but allows configuring the CAS threshold
    pub unsafe fn open_with_cas_threshold(main_dir: PathBuf, cas_threshold: usize) -> anyhow::Result<Self> {
        let buffers_dir = main_dir.join("buffers");
        let cas_dir = main_dir.join("cas");

        // Cleanup buffers.
        let _ = fs::remove_dir(&buffers_dir);
        fs::create_dir_all(&buffers_dir)?;
        fs::create_dir_all(&cas_dir)?;

        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(TABLES_COUNT)
                .map_size(DEFAULT_MAP_SIZE)
                .open(&main_dir)
        }?;

        migrations::run(&env)?;
        let mut wtxn = env.write_txn()?;
        let tables = Tables::new(&env, &mut wtxn)?;
        wtxn.commit()?;

        let cas = FileSystemCAS::new(cas_dir);

        let db = LmDB {
            env,
            tables,
            max_chunk_size: Self::max_chunk_size(),
            cas,
            cas_threshold,
            test_dir: None,
        };

        Ok(db)
    }

    /// Store blob data, automatically choosing between inline storage and CAS
    pub fn store_blob(&self, data: &[u8]) -> Result<BlobStorage> {
        if data.len() <= self.cas_threshold {
            // Store small data inline in LMDB
            Ok(BlobStorage::Inline(data.to_vec()))
        } else {
            // Store large data in CAS
            let content_id = self.cas.store(data)?;
            Ok(BlobStorage::CasRef(content_id))
        }
    }

    /// Store string blob data
    pub fn store_string_blob(&self, data: &str) -> Result<BlobStorage> {
        if data.len() <= self.cas_threshold {
            Ok(BlobStorage::Inline(data.as_bytes().to_vec()))
        } else {
            let content_id = self.cas.store_string(data)?;
            Ok(BlobStorage::CasRef(content_id))
        }
    }

    /// Retrieve blob data from storage
    pub fn retrieve_blob(&self, storage: &BlobStorage) -> Result<Vec<u8>> {
        match storage {
            BlobStorage::Inline(data) => Ok(data.clone()),
            BlobStorage::CasRef(content_id) => {
                self.cas.retrieve(content_id).map_err(|e| anyhow::anyhow!(e))
            }
        }
    }

    /// Retrieve string blob data from storage
    pub fn retrieve_string_blob(&self, storage: &BlobStorage) -> Result<String> {
        match storage {
            BlobStorage::Inline(data) => {
                String::from_utf8(data.clone()).map_err(|e| anyhow::anyhow!(e))
            }
            BlobStorage::CasRef(content_id) => {
                self.cas.retrieve_string(content_id).map_err(|e| anyhow::anyhow!(e))
            }
        }
    }

    /// Get storage statistics
    pub fn blob_storage_stats(&self) -> BlobStorageStats {
        // In a real implementation, you'd track these metrics
        BlobStorageStats {
            inline_count: 0,
            cas_count: 0,
            total_inline_size: 0,
            total_cas_size: 0,
            cas_threshold: self.cas_threshold,
        }
    }

    /// Garbage collect unused CAS entries
    /// This is a placeholder - you'd need to implement reference tracking
    pub fn gc_cas_entries(&self) -> Result<GcStats> {
        // In a real implementation, you would:
        // 1. Scan all LMDB entries to find CAS references
        // 2. Compare with actual CAS files
        // 3. Remove unreferenced CAS files
        Ok(GcStats {
            files_removed: 0,
            bytes_freed: 0,
        })
    }

    /// calculate optimal chunk size:
    /// - <https://lmdb.readthedocs.io/en/release/#storage-efficiency-limits>
    /// - <https://github.com/lmdbjava/benchmarks/blob/master/results/20160710/README.md#test-2-determine-24816-kb-byte-values>
    fn max_chunk_size() -> usize {
        let page_size = page_size::get();

        // - 16 bytes Header  per page (LMDB)
        // - Each page has to contain 2 records
        // - 8 bytes per record (LMDB) (empirically, it seems to be 10 not 8)
        // - 12 bytes key:
        //      - timestamp : 8 bytes
        //      - chunk index: 4 bytes
        ((page_size - 16) / 2) - (8 + 2) - 12
    }
}

#[derive(Debug)]
pub struct BlobStorageStats {
    pub inline_count: usize,
    pub cas_count: usize,
    pub total_inline_size: usize,
    pub total_cas_size: usize,
    pub cas_threshold: usize,
}

#[derive(Debug)]
pub struct GcStats {
    pub files_removed: usize,
    pub bytes_freed: usize,
}

// Example usage and helper methods
impl LmDB {
    /// Example: Store a document with automatic blob handling
    pub fn store_document(&self, key: &str, content: &str, metadata: &str) -> Result<()> {
        let content_storage = self.store_string_blob(content)?;
        let metadata_storage = self.store_string_blob(metadata)?;

        let document = Document {
            key: key.to_string(),
            content: content_storage,
            metadata: metadata_storage,
            created_at: std::time::SystemTime::now(),
        };

        // Store document metadata in LMDB (you'd need to implement this)
        // self.store_document_metadata(&document)?;

        Ok(())
    }

    /// Example: Retrieve a document with automatic blob handling
    pub fn get_document(&self, key: &str) -> Result<Option<Document>> {
        // Retrieve document metadata from LMDB (you'd need to implement this)
        // let document_meta = self.get_document_metadata(key)?;

        // For demonstration, return None
        Ok(None)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Document {
    pub key: String,
    pub content: BlobStorage,
    pub metadata: BlobStorage,
    pub created_at: std::time::SystemTime,
}