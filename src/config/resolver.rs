//! TOML loading and field finalisation for [`super::Config`].

use std::path::{Path, PathBuf};

use crate::config::defaults::{detect_cdb, CDB_ENV_VAR};
use crate::config::model::Config;
use crate::error::{Error, Result};

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut cfg: Config = match path {
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .map_err(|e| Error::Config(format!("read {}: {e}", p.display())))?;
                toml::from_str(&text)?
            }
            None => Config::default(),
        };
        cfg.finalize()?;
        Ok(cfg)
    }

    fn finalize(&mut self) -> Result<()> {
        if self.debugger.cdb_path.is_none() {
            if let Ok(env) = std::env::var(CDB_ENV_VAR) {
                if !env.trim().is_empty() {
                    self.debugger.cdb_path = Some(PathBuf::from(env));
                }
            }
        }
        if self.debugger.cdb_path.is_none() {
            self.debugger.cdb_path = detect_cdb();
        }
        Ok(())
    }
}
