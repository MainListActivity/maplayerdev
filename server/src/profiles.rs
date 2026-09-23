//! Codex account profiles: one CODEX_HOME per profile so each carries its own
//! auth.json + config.toml. Switching accounts = spawning with a different
//! CODEX_HOME.

use crate::config::Config;
use crate::proto::{LoginInstruction, Profile, ProfileNewResult};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Serialize, Deserialize)]
struct ProfileMeta {
    name: String,
    credential: String,
}

pub struct ProfileManager {
    root: PathBuf,
}

impl ProfileManager {
    pub fn new(cfg: &Config) -> Self {
        Self {
            root: cfg.profiles_root.clone(),
        }
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn validate_name(name: &str) -> Result<()> {
        let ok = !name.is_empty()
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if ok {
            Ok(())
        } else {
            bail!("profile name must be 1-64 chars of [a-zA-Z0-9_-]")
        }
    }

    pub async fn list(&self) -> Result<Vec<Profile>> {
        let mut out = Vec::new();
        let mut rd = match fs::read_dir(&self.root).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        while let Some(ent) = rd.next_entry().await? {
            let meta_path = ent.path().join("meta.json");
            if let Ok(bytes) = fs::read(&meta_path).await {
                if let Ok(meta) = serde_json::from_slice::<ProfileMeta>(&bytes) {
                    out.push(Profile {
                        codex_home: self.dir(&meta.name).to_string_lossy().into(),
                        name: meta.name,
                        credential: meta.credential,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Create an isolated CODEX_HOME for the profile. Returns a login
    /// instruction the client surfaces to the user — credentials are never
    /// sent over the wire; login completes on the host (desktop) side.
    pub async fn create(&self, name: &str, credential: &str) -> Result<ProfileNewResult> {
        Self::validate_name(name)?;
        if credential != "chatgpt" && credential != "api-key" {
            bail!("credential must be chatgpt|api-key");
        }
        let dir = self.dir(name);
        fs::create_dir_all(&dir).await?;
        let meta = ProfileMeta {
            name: name.into(),
            credential: credential.into(),
        };
        fs::write(dir.join("meta.json"), serde_json::to_vec_pretty(&meta)?).await?;
        // CODEX_HOME must exist before codex touches it; seed an empty config.
        let cfg = dir.join("config.toml");
        if !cfg.exists() {
            fs::write(&cfg, "").await?;
        }
        let login = match credential {
            "api-key" => LoginInstruction {
                kind: "api_key_prompt".into(),
                text: format!(
                    "Run on host: printenv OPENAI_API_KEY | CODEX_HOME={} codex login --with-api-key",
                    dir.display()
                ),
            },
            _ => LoginInstruction {
                kind: "url".into(),
                text: format!(
                    "Run on host: CODEX_HOME={} codex login (opens browser)",
                    dir.display()
                ),
            },
        };
        Ok(ProfileNewResult {
            name: name.into(),
            codex_home: dir.to_string_lossy().into(),
            login,
        })
    }

    /// Resolve profile name → CODEX_HOME. `None` falls back to the server
    /// default profile, then to the user's own ~/.codex.
    pub async fn codex_home(&self, name: Option<&str>, cfg: &Config) -> Result<Option<PathBuf>> {
        let name = match name {
            Some(n) => Some(n.to_string()),
            None => cfg.server_config().await.default_profile,
        };
        match name {
            Some(n) => {
                let dir = self.dir(&n);
                if !dir.join("meta.json").exists() {
                    bail!("unknown profile: {n}");
                }
                Ok(Some(dir))
            }
            None => Ok(None), // agent uses its own default CODEX_HOME
        }
    }
}
