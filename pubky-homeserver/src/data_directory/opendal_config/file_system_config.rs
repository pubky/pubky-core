use std::path::PathBuf;

const DATA_DIRECTORY_PLACEHOLDER: &str = "{DATA_DIRECTORY}";

/// The file system config. Files are stored on the local file system.
/// The root_directory is the path the files are stored in.
/// `{DATA_DIRECTORY}` can be used as a variable in the root_directory. 
/// It is replaced with the data directory path.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSystemConfig {
    /// The root directory to use.
    #[serde(default = "default_root_directory")]
    pub root_directory: String,
}

fn default_root_directory() -> String {
    format!("{DATA_DIRECTORY_PLACEHOLDER}/data/files/")
}

impl Default for FileSystemConfig {
    fn default() -> Self {
        Self {
            root_directory: default_root_directory(),
        }
    }
}


impl FileSystemConfig {
    /// Expands the `DATA_DIRECTORY_PLACEHOLDER` variable with the given data directory.
    pub fn expand_with_data_directory(&mut self, data_directory: &PathBuf) {
        if self.root_directory.starts_with(DATA_DIRECTORY_PLACEHOLDER) {
            let mut path = self.root_directory.replace(DATA_DIRECTORY_PLACEHOLDER, "");
            // Remove the first character if it exists (usually "/"). Otherwise the join will replace the directory instead of appending.
            if !path.is_empty() {
                path = path.chars().skip(1).collect();
            }
            self.root_directory =  data_directory.join(path).to_str().unwrap_or_default().to_string();
        }
    }

    /// Returns the builder for the file system. This will create the directory if it doesn't exist.
    /// Make sure to call `expand_with_data_directory` before using this method.
    pub fn to_builder(&mut self) -> anyhow::Result<opendal::services::Fs> {
        let path = PathBuf::from(&self.root_directory);
        if !path.exists() {
            std::fs::create_dir_all(&path)?;
        }
        let builder = opendal::services::Fs::default()
        .root(&self.root_directory);
        Ok(builder)
    }
}