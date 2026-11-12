//! Uygulanan tweak'lerin eski durumlarının kaydı. Geri alma yalnızca bu kayıttan yapılır.
//!
//! Konum: Windows `%LOCALAPPDATA%\Skadi`, macOS `~/Library/Application Support/Skadi`,
//! Linux `$XDG_STATE_HOME/skadi` (yoksa `~/.local/state/skadi`).

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::action::Previous;
use crate::{
    error::{Error, Result},
    platform::Os,
};

const FILE_NAME: &str = "tweaks-state.toml";
const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub key: String,
    pub previous: Previous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TweakRecord {
    pub applied_at: u64,
    /// Bu tweak'e en son dokunan Skadi oturumu; "son oturumu geri al" bununla çalışır.
    #[serde(default)]
    pub session: u64,
    pub actions: Vec<ActionRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    tweaks: BTreeMap<String, TweakRecord>,
}

#[derive(Debug)]
pub struct StateStore {
    path: PathBuf,
    state: State,
    /// Şifreli kayıt için işletim sistemi; `None` ise kayıt düz metin yazılır.
    encrypt_for: Option<Os>,
    /// Bu süreçte yapılan kayıtların oturum kimliği (açılış zamanı).
    session: u64,
}

impl StateStore {
    /// Varsayılan konumdaki kaydı açar; dosya yoksa boş başlar.
    pub fn open(os: Os) -> Result<Self> {
        let dir = crate::paths::data_dir(os).ok_or_else(|| Error::State("no home directory".into()))?;
        let mut store = Self::open_at_with(dir.join(FILE_NAME), Some(os))?;
        if crate::settings::Settings::load(os)?.state.encrypt {
            store.encrypt_for = Some(os);
        }
        Ok(store)
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        Self::open_at_with(path, None)
    }

    fn open_at_with(path: PathBuf, os: Option<Os>) -> Result<Self> {
        let state = match fs::metadata(&path) {
            Ok(meta) if meta.len() > MAX_STATE_BYTES => return Err(Error::State("state file is too large".into())),
            Ok(_) => {
                let mut data = fs::read(&path).map_err(|e| Error::State(e.to_string()))?;
                if crate::vault::is_encrypted(&data) {
                    let os = os.ok_or_else(|| Error::State("encrypted state needs the key store".into()))?;
                    data = crate::vault::decrypt(&crate::vault::state_key(os, false)?, &data)?;
                }
                let text = String::from_utf8(data).map_err(|e| Error::State(e.to_string()))?;
                toml::from_str(&text).map_err(|e| Error::State(e.to_string()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(Error::State(e.to_string())),
        };
        Ok(Self {
            path,
            state,
            encrypt_for: None,
            session: now(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, tweak: &str) -> Option<&TweakRecord> {
        self.state.tweaks.get(tweak)
    }

    /// Kaydı olan tüm tweak'ler.
    /// Kaydı olan tweak'ler ve son uygulanma zamanları (Unix saniyesi).
    pub fn applied_times(&self) -> Vec<(String, u64)> {
        self.state
            .tweaks
            .iter()
            .map(|(id, record)| (id.clone(), record.applied_at))
            .collect()
    }

    pub fn recorded(&self) -> Vec<String> {
        self.state.tweaks.keys().cloned().collect()
    }

    /// En son oturumda uygulanan tweak'ler.
    pub fn last_session(&self) -> Vec<String> {
        let latest = self.state.tweaks.values().map(|r| r.session).max().unwrap_or(0);
        self.state
            .tweaks
            .iter()
            .filter(|(_, r)| r.session == latest)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn is_recorded(&self, tweak: &str) -> bool {
        self.state.tweaks.contains_key(tweak)
    }

    /// Yeni eylemleri ekler. Daha önce kaydedilmiş bir eylemin **ilk** eski durumu korunur;
    /// böylece aynı tweak iki kez uygulansa da geri alma özgün duruma döner.
    pub fn record(&mut self, tweak: &str, actions: Vec<ActionRecord>) {
        if actions.is_empty() {
            return;
        }
        let record = self
            .state
            .tweaks
            .entry(tweak.to_owned())
            .or_insert_with(|| TweakRecord {
                applied_at: now(),
                session: 0,
                actions: Vec::new(),
            });
        record.session = self.session;
        for action in actions {
            if !record.actions.iter().any(|a| a.key == action.key) {
                record.actions.push(action);
            }
        }
    }

    /// Geri alınamayan eylemleri kayıtta bırakır; liste boşsa kaydı siler.
    pub fn replace(&mut self, tweak: &str, remaining: Vec<ActionRecord>) {
        if remaining.is_empty() {
            self.state.tweaks.remove(tweak);
        } else if let Some(record) = self.state.tweaks.get_mut(tweak) {
            record.actions = remaining;
        }
    }

    /// Önce geçici dosyaya yazar, sonra yerine taşır: yarım kalmış bir yazma kaydı bozamaz.
    pub fn save(&self) -> Result<()> {
        let io = |e: std::io::Error| Error::State(format!("{}: {e}", self.path.display()));
        let dir = self
            .path
            .parent()
            .ok_or_else(|| Error::State("invalid state path".into()))?;
        fs::create_dir_all(dir).map_err(io)?;
        let text = toml::to_string_pretty(&self.state).map_err(|e| Error::State(e.to_string()))?;
        let bytes = match self.encrypt_for {
            Some(os) => crate::vault::encrypt(&crate::vault::state_key(os, true)?, text.as_bytes())?,
            None => text.into_bytes(),
        };
        let temp = self.path.with_extension("toml.tmp");
        let mut file = create_private(&temp).map_err(io)?;
        file.write_all(&bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        fs::rename(&temp, &self.path).map_err(io)
    }
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    // SECURITY FIX: CWE-276 kayıt sistem yapılandırmasını içerir; yalnızca sahibi okuyabilir.
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    // %LOCALAPPDATA% zaten kullanıcıya özel ACL ile korunur.
    fs::File::create(path)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("skadi-test-{}-{name}", std::process::id()))
            .join(FILE_NAME)
    }

    #[test]
    fn round_trips_and_keeps_first_snapshot() {
        let path = temp_path("roundtrip");
        let mut store = StateStore::open_at(path.clone()).unwrap();
        let first = ActionRecord {
            key: "registry:HKCU\\X\\V".into(),
            previous: Previous::Registry {
                value: None,
                created: None,
            },
        };
        store.record("t", vec![first.clone()]);
        store.record(
            "t",
            vec![
                ActionRecord {
                    key: first.key.clone(),
                    previous: Previous::Registry {
                        value: Some("0".into()),
                        created: None,
                    },
                },
                ActionRecord {
                    key: "service:X".into(),
                    previous: Previous::Service { start: 2 },
                },
            ],
        );
        store.save().unwrap();

        let reopened = StateStore::open_at(path.clone()).unwrap();
        let record = reopened.get("t").unwrap();
        assert_eq!(record.actions[0], first);
        assert_eq!(record.actions.len(), 2);

        let mut reopened = reopened;
        reopened.replace("t", Vec::new());
        assert!(!reopened.is_recorded("t"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rejects_corrupt_state() {
        let path = temp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "tweaks = 5").unwrap();
        assert!(StateStore::open_at(path.clone()).is_err());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
