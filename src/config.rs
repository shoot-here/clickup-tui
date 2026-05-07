use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

const APP_DIR: &str = "clickup-tui";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub api_token: String,
}

#[derive(Debug)]
pub enum LoadError {
    /// No config file exists yet — caller should run the welcome flow.
    Missing,
    /// File exists but couldn't be parsed or token is empty.
    Invalid(anyhow::Error),
}

impl Config {
    pub fn load() -> std::result::Result<Self, LoadError> {
        let path = match config_path() {
            Ok(p) => p,
            Err(e) => return Err(LoadError::Invalid(e)),
        };
        if !path.exists() {
            return Err(LoadError::Missing);
        }
        let raw = match fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))
        {
            Ok(s) => s,
            Err(e) => return Err(LoadError::Invalid(e)),
        };
        let config: Config = match toml::from_str(&raw).context("parse config.toml") {
            Ok(c) => c,
            Err(e) => return Err(LoadError::Invalid(e)),
        };
        if config.api_token.trim().is_empty() {
            return Err(LoadError::Invalid(anyhow!(
                "api_token is empty in {}",
                path.display()
            )));
        }
        Ok(config)
    }

    /// Write the config file with `chmod 600` on Unix. Creates parent dirs.
    pub fn save_token(token: &str) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let body = format!("api_token = \"{}\"\n", token.replace('\\', "\\\\").replace('"', "\\\""));
        fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

fn config_path() -> Result<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join(APP_DIR).join(CONFIG_FILE));
        }
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow!("can't locate $HOME"))?;
    Ok(home.join(".config").join(APP_DIR).join(CONFIG_FILE))
}
