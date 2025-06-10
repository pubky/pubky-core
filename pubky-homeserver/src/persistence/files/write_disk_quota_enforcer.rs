use crate::persistence::files::FileIoError;
use crate::persistence::lmdb::LmDB;
use crate::shared::webdav::EntryPath;

/// Checks if the content-size hint already exceeds the quota.
/// This is not reliable because the user might supply a fake size hint
/// but it can be used for error messages and to fail the upload early.
pub fn is_size_hint_exceeding_quota(
    content_size_hint: u64,
    db: &LmDB,
    path: &EntryPath,
    max_allowed_bytes: u64,
) -> Result<bool, FileIoError> {
    let existing_entry_bytes = db.get_entry_content_length_default_zero(path)?;
    let user_already_used_bytes = match db.get_user_data_usage(path.pubkey())? {
        Some(bytes) => bytes,
        None => return Err(FileIoError::NotFound),
    };

    Ok(
        user_already_used_bytes + content_size_hint.saturating_sub(existing_entry_bytes)
            > max_allowed_bytes,
    )
}
