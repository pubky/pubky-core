use crate::commands::Commands;
use clap::Parser;
use std::path::PathBuf;

fn default_config_dir_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".homeserver")
}

fn validate_config_dir_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.exists() && path.is_file() {
        return Err(format!("Given path is not a directory: {}", path.display()));
    }
    Ok(path)
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[clap(short, long, default_value_os_t = default_config_dir_path(), value_parser = validate_config_dir_path)]
    pub data_dir: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}
