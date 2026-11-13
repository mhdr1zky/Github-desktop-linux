use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const MAX_PREFERENCES_BYTES: u64 = 64 * 1024;
const MAX_RECENT: usize = 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub schema_version: u32,
    pub recent: Vec<PathBuf>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: 1,
            recent: Vec::new(),
        }
    }
}

impl Preferences {
    pub fn remember(&mut self, root: PathBuf) {
        self.recent.retain(|path| path != &root);
        self.recent.insert(0, root);
        self.recent.truncate(MAX_RECENT);
    }
}

pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|root| root.join("gitrust-desktop/preferences.json"))
}

pub fn load(path: &Path) -> Result<Preferences> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(err) => return Err(err.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_PREFERENCES_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PREFERENCES_BYTES as usize {
        return Err(Error::Invalid(
            "Preferences exceed the 64 KiB limit. Existing file was left unchanged.".into(),
        ));
    }
    let preferences: Preferences = serde_json::from_slice(&bytes)?;
    if preferences.schema_version != 1 || preferences.recent.len() > MAX_RECENT {
        return Err(Error::Unsupported(
            "Unsupported preferences schema. Existing file was left unchanged.".into(),
        ));
    }
    Ok(preferences)
}

pub fn save(path: &Path, preferences: &Preferences) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Preferences require a parent directory.".into()))?;
    let bytes = serde_json::to_vec_pretty(preferences)?;
    if bytes.len() > MAX_PREFERENCES_BYTES as usize {
        return Err(Error::Invalid("Preferences exceed the size limit.".into()));
    }
    std::fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".preferences-{}-{nonce}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        if path.exists() {
            std::fs::copy(path, path.with_extension("json.bak"))?;
        }
        std::fs::rename(&temporary, path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
