use std::sync::Arc;

use opendal::Operator;
use tempfile::TempDir;

/// A provider of operators for testing.
///
/// Provides:
/// - A filesystem operator
/// - A memory operator
/// - A GCS operator (if the environment variables are set)
///
/// The GCS operator is optional and will be None if the environment variables are not set.
/// GCS environment variables:
/// - TEST_GOOGLE_APPLICATION_CREDENTIALS: The path to the GCS credentials file.
/// - TEST_GCS_BUCKET: The name of the GCS bucket.
#[derive(Clone)]
pub struct OpendalProviders {
    pub fs_operator: Operator,
    /// The temporary directory for the fs operator.
    #[allow(dead_code)]
    fs_tmp_dir: Arc<TempDir>,
    pub gcs_operator: Option<Operator>,
    pub memory_operator: Operator,
}

impl OpendalProviders {
    /// Create a new instance of the OperatorTestProviders.
    pub fn new() -> Self {
        let (fs_operator, fs_tmp_dir) = get_fs_operator();
        Self {
            fs_operator: fs_operator,
            fs_tmp_dir: Arc::new(fs_tmp_dir),
            gcs_operator: get_gcs_operator(),
            memory_operator: get_memory_operator(),
        }
    }

    /// Get all operators.
    pub fn operators(&self) -> Vec<&Operator> {
        let mut operators = vec![&self.fs_operator, &self.memory_operator];
        if let Some(gcs_operator) = &self.gcs_operator {
            operators.push(gcs_operator);
        }
        operators
    }

    /// Check if the GCS operator is available.
    /// This depends on the environment variables being set. See OpendalProviders docs for more details.
    pub fn is_gcs_available(&self) -> bool {
        self.gcs_operator.is_some()
    }
}

fn get_fs_operator() -> (Operator, TempDir) {
    let tmp_dir = tempfile::tempdir().unwrap();
    let s = tmp_dir.path().to_str().unwrap();
    let builder = opendal::services::Fs::default().root(s);
    let operator = opendal::Operator::new(builder).unwrap().finish();
    (operator, tmp_dir)
}

fn get_gcs_operator() -> Option<Operator> {
    let credential_path = match std::env::var("TEST_GOOGLE_APPLICATION_CREDENTIALS") {
        Ok(path) => path,
        Err(_) => return None,
    };
    let bucket_name = match std::env::var("TEST_GCS_BUCKET") {
        Ok(path) => path,
        Err(_) => return None,
    };
    let builder = opendal::services::Gcs::default()
        .bucket(&bucket_name)
        .credential_path(&credential_path);
    let operator = opendal::Operator::new(builder).unwrap().finish();
    Some(operator)
}

fn get_memory_operator() -> Operator {
    let builder = opendal::services::Memory::default();
    let operator = opendal::Operator::new(builder).unwrap().finish();
    operator
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_operator_test_providers() {
        let providers = OpendalProviders::new();
        let operators = providers.operators();
        for operator in operators {
            
            let _ = operator.read("test").await.unwrap();
        }
    }
}
