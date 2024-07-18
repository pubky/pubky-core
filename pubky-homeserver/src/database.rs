use std::fs;
use std::path::Path;

use heed::{Env, EnvOpenOptions};

#[derive(Debug, Clone)]
pub struct DB {
    env: Env,
}

impl DB {
    pub fn open(storage: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(storage).unwrap();

        let env = unsafe { EnvOpenOptions::new().open(storage) }?;

        Ok(DB { env })
    }
}
