use crate::shared::webdav::EntryPath;
use opendal::Result;

/// Helper function to ensure that the path is a valid entry path aka
/// starts with a pubkey.
/// Returns the entry path if it is valid, otherwise returns an error.
pub(super) fn ensure_valid_path(path: &str) -> Result<EntryPath, opendal::Error> {
    let path: EntryPath = match path.parse() {
        Ok(path) => path,
        Err(e) => {
            return Err(opendal::Error::new(
                opendal::ErrorKind::PermissionDenied,
                e.to_string(),
            ));
        }
    };
    Ok(path)
}
