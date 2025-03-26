use super::{ConfigToml, DataDir};
use pkarr::Keypair;


/// The full configuration read from the data directory.
#[derive(Debug, Clone)]
pub(crate) struct FullConfig {
    pub(crate) toml: ConfigToml,
    pub(crate) keypair: Keypair,
    pub(crate) data_dir: DataDir,
}

impl TryFrom<DataDir> for FullConfig {
    type Error = anyhow::Error;

    fn try_from(dir: DataDir) -> Result<Self, Self::Error> {
        dir.ensure_data_dir_exists_and_is_writable()?;
        let conf = dir.read_or_create_config_file()?;
        let keypair = dir.read_or_create_keypair()?;

        Ok(FullConfig {
            toml: conf,
            keypair,
            data_dir: dir,
        })
    }
}