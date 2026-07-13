//! Shared Pubky storage path helpers.

/// Storage root for public, world-readable data.
pub const PUBLIC_ROOT: &str = "/pub/";

/// Storage root for private data.
pub const PRIVATE_ROOT: &str = "/priv/";

/// Returns whether a normalized storage path is under [`PRIVATE_ROOT`].
pub fn is_private_path(path: &str) -> bool {
    path.starts_with(PRIVATE_ROOT)
}

/// Classifies a user-supplied path filter after WebDAV-style normalization.
pub fn is_private_path_filter(path: &str) -> bool {
    let Some(path) = normalize_path_filter(path) else {
        return false;
    };
    is_private_path(&path)
}

fn normalize_path_filter(path: &str) -> Option<String> {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }

    if segments.is_empty() {
        return None;
    }

    let mut normalized = format!("/{}", segments.join("/"));
    if (path.ends_with('/') || path.ends_with("..")) && !normalized.ends_with('/') {
        normalized.push('/');
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::is_private_path_filter;

    #[test]
    fn private_path_filter_matches_normalized_priv_directory() {
        let cases = [
            ("", false),
            ("/pub/", false),
            ("/priv", false),
            ("/privstuff/x", false),
            ("/priv/", true),
            ("priv/x", true),
            ("/pub/../priv/secret/", true),
            ("/../../priv/secret/", false),
        ];

        for (path, expected) in cases {
            assert_eq!(is_private_path_filter(path), expected, "{path}");
        }
    }
}
