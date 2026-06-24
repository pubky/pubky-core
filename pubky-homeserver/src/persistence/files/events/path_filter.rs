//! Authorized path filter for the event stream, shared by the route and the repository.

use sea_query::{Expr, LikeExpr, SimpleExpr};

use crate::shared::webdav::WebDavPath;

use super::events_repository::{EventIden, EVENT_TABLE};

/// A single authorized path filter for the event stream.
///
/// A file filter matches only its exact path, while a directory filter (trailing `/`)
/// matches that directory and all of its descendants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathFilter {
    /// Exact-match filter for a file path (no trailing `/`).
    File(String),
    /// Prefix filter for a directory path (trailing `/`) and its descendants.
    Dir(String),
}

impl PathFilter {
    /// Build a filter from a request path.
    pub fn from_path(path: &WebDavPath) -> Self {
        if path.is_directory() {
            PathFilter::Dir(path.as_str().to_string())
        } else {
            PathFilter::File(path.as_str().to_string())
        }
    }

    /// Whether `path` is selected by this filter.
    pub fn matches(&self, path: &str) -> bool {
        match self {
            PathFilter::File(p) => path == p,
            PathFilter::Dir(p) => path.starts_with(p),
        }
    }

    /// SQL predicate selecting rows whose `path` column matches this filter.
    /// File filters use exact equality; directory filters use an escaped
    /// `LIKE '<dir>%'` so `_`/`%`/`\` in the stored prefix are matched
    /// literally and cannot widen the match.
    pub(super) fn to_condition(&self) -> SimpleExpr {
        match self {
            PathFilter::File(p) => Expr::col((EVENT_TABLE, EventIden::Path)).eq(p.clone()),
            PathFilter::Dir(p) => {
                let escaped = p
                    .replace('\\', "\\\\")
                    .replace('_', "\\_")
                    .replace('%', "\\%");
                let like_pattern = format!("{escaped}%");
                Expr::col((EVENT_TABLE, EventIden::Path))
                    .like(LikeExpr::new(like_pattern).escape('\\'))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_filter_matches_file_and_dir_boundaries() {
        let file = PathFilter::File("/priv/app".to_string());
        assert!(file.matches("/priv/app"));
        assert!(!file.matches("/priv/app/x")); // a file scope has no descendants
        assert!(!file.matches("/priv/app-evil")); // sibling-prefix
        assert!(!file.matches("/priv/ap"));

        let dir = PathFilter::Dir("/priv/app/".to_string());
        assert!(dir.matches("/priv/app/"));
        assert!(dir.matches("/priv/app/x"));
        assert!(dir.matches("/priv/app/sub/y"));
        assert!(!dir.matches("/priv/app")); // the parent file
        assert!(!dir.matches("/priv/app-evil/x")); // sibling-prefix
        assert!(!dir.matches("/priv/other/x"));
    }

    #[test]
    fn path_filter_from_path_distinguishes_file_and_dir() {
        assert_eq!(
            PathFilter::from_path(&WebDavPath::new("/priv/app/").unwrap()),
            PathFilter::Dir("/priv/app/".into())
        );
        assert_eq!(
            PathFilter::from_path(&WebDavPath::new("/priv/app").unwrap()),
            PathFilter::File("/priv/app".into())
        );
    }
}
