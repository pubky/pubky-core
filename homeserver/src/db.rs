use std::fs;
use std::path::Path;

use heed::{Env, EnvOpenOptions};

use pk_common::{Error, Result};

#[derive(Debug, Clone)]
pub struct DB {
    env: Env,
}

impl DB {
    pub fn open(storage: &Path) -> Result<Self> {
        fs::create_dir_all(&storage).unwrap();

        let env = unsafe {
            EnvOpenOptions::new()
                .open(storage)
                .map_err(|_| Error::Generic("could not open databas".to_string()))?
        };

        Ok(DB { env })
    }
}
