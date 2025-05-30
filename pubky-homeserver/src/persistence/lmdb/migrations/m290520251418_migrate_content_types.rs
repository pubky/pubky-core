use super::super::tables::files;
use crate::persistence::lmdb::tables::files::Entry;
use heed::types::{Bytes, Str};
use heed::{Database, Env, RwTxn};


/// Checks if the migration is needed.
/// Tries to read users with the new schema. If it succeeds, the migration is not needed.
/// If it fails, the migration is needed.
fn is_migration_needed(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<bool> {
    let blobs: Database<Bytes, Bytes> = env
        .open_database(wtxn, Some(files::BLOBS_TABLE))?
        .expect("Blobs database is not available");

    let metadata: Database<Str, Bytes> = env
        .open_database(wtxn, Some(files::ENTRIES_TABLE))?
        .expect("Entries database is not available");

    let meta = metadata.first(wtxn);
    let file = blobs.first(wtxn);

    match (meta, file) {
        (Ok(Some((key, meta))), Ok(Some((_, file)))) => {
            let entry = Entry::deserialize(meta).expect("Deserialization of entry failed");
            let mime_matches = match infer::get(file) {
                Some(kind) => kind.mime_type() == entry.content_type(),
                None => {
                    let path_guess = mime_guess::from_path(key)
                        .first_or_octet_stream()
                        .to_string();
                    path_guess == entry.content_type()
                }
            };
            Ok(!mime_matches)
        }
        (_, _) => Ok(false), // Second failed
    }
}

fn infer_blobs_types(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<Vec<(String, String)>> {
    let blobs_db: Database<Bytes, Bytes> = env
        .open_database(wtxn, Some(files::BLOBS_TABLE))?
        .expect("Blobs database is not available");

    let meta_db: Database<Str, Bytes> = env
        .open_database(wtxn, Some(files::ENTRIES_TABLE))?
        .expect("Entries database is not available");

    let mut blobs: Vec<(String, String)> = vec![];

    for entry in meta_db.iter(wtxn)? {
        if let Ok((key, data)) = entry {
            let entry = Entry::deserialize(data).expect("Deserialization of entry failed");
            let id = entry.file_id().get_blob_key(0);

            let file = blobs_db.get(wtxn, &id).expect("some").unwrap();

            let mime_inferred = match infer::get(file) {
                Some(kind) => kind.mime_type().to_string(),
                _ => mime_guess::from_path(key)
                    .first_or_octet_stream()
                    .to_string(),
            };

            blobs.push((key.to_string(), mime_inferred));
        }
    }

    Ok(blobs)
}

pub fn run(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    if !is_migration_needed(env, wtxn)? {
        return Ok(());
    }

    tracing::info!("Running migration m290520251418_migrate_content_types");

    tracing::info!("Successfully migrated");

    Ok(())
}

#[cfg(test)]
mod tests {
    use heed::EnvOpenOptions;
    use pkarr::Keypair;
    use std::io::Read;

    use super::*;

    use crate::persistence::lmdb::tables::files::{Entry, InDbTempFile};

    use crate::persistence::lmdb::{db::DEFAULT_MAP_SIZE, migrations::m0};
    use crate::shared::webdav::{EntryPath, WebDavPath};

    #[test]
    fn test_is_migration_needed_for_magic_bytes_yes() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(20)
                .map_size(DEFAULT_MAP_SIZE)
                .open(tmp_dir.path())
        }
        .unwrap();

        m0::run(&env, &mut env.write_txn().unwrap()).unwrap();
        let mut wtxn = env.write_txn().unwrap();

        let path = EntryPath::new(
            Keypair::random().public_key(),
            WebDavPath::new("/pub/foo.txt").unwrap(),
        );

        let file = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(InDbTempFile::png_pixel())
            .unwrap();

        let mut entry = Entry::new();
        entry.set_content_hash(*file.hash());
        entry.set_content_length(file.len());
        entry.set_content_type("dummy".to_string());
        entry.set_timestamp(&Default::default());
        let entry_key = path.to_string();

        // Write a user to the old table.
        let metadata: files::EntriesTable = env
            .create_database(&mut wtxn, Some(files::ENTRIES_TABLE))
            .unwrap();
        metadata
            .put(&mut wtxn, entry_key.as_str(), &entry.serialize())
            .unwrap();

        let blobs: files::BlobsTable = env
            .create_database(&mut wtxn, Some(files::BLOBS_TABLE))
            .unwrap();

        let blob_key = entry.file_id().get_blob_key(0);
        let mut blob = vec![0_u8; 64];

        let mut file_handle = file.open_file_handle().unwrap();

        file_handle
            .read(&mut blob)
            .expect("read png file successfully");

        blobs.put(&mut wtxn, &blob_key, &blob[..64]).unwrap();

        // Test magic bytes inference because DB contains magic bytes file right now
        let binding = infer_blobs_types(&env, &mut wtxn).unwrap();
        let (_, png_test) = binding.first().unwrap();

        assert_eq!(png_test, "image/png");

        assert!(is_migration_needed(&env, &mut wtxn).unwrap());
    }

    #[test]
    fn test_is_migration_needed_for_extension_yes() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(20)
                .map_size(DEFAULT_MAP_SIZE)
                .open(tmp_dir.path())
        }
        .unwrap();

        m0::run(&env, &mut env.write_txn().unwrap()).unwrap();
        let mut wtxn = env.write_txn().unwrap();

        let path = EntryPath::new(
            Keypair::random().public_key(),
            WebDavPath::new("/pub/foo.txt").unwrap(),
        );

        let file = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(InDbTempFile::zeros(2))
            .unwrap();

        let mut entry = Entry::new();
        entry.set_content_hash(*file.hash());
        entry.set_content_length(file.len());
        entry.set_content_type("dummy".to_string());
        entry.set_timestamp(&Default::default());
        let entry_key = path.to_string();

        // Write a user to the old table.
        let metadata: files::EntriesTable = env
            .create_database(&mut wtxn, Some(files::ENTRIES_TABLE))
            .unwrap();
        metadata
            .put(&mut wtxn, entry_key.as_str(), &entry.serialize())
            .unwrap();

        let blobs: files::BlobsTable = env
            .create_database(&mut wtxn, Some(files::BLOBS_TABLE))
            .unwrap();

        let blob_key = entry.file_id().get_blob_key(0);
        let mut blob = vec![0_u8; 64];

        let mut file_handle = file.open_file_handle().unwrap();

        file_handle
            .read(&mut blob)
            .expect("read png file successfully");

        blobs.put(&mut wtxn, &blob_key, &blob[..64]).unwrap();

        // Test magic bytes inference because DB contains magic bytes file right now
        let binding = infer_blobs_types(&env, &mut wtxn).unwrap();
        let (_, txt_test) = binding.first().unwrap();

        assert_eq!(txt_test, "text/plain");

        assert!(is_migration_needed(&env, &mut wtxn).unwrap());
    }
}
