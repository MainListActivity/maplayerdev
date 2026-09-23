use anyhow::{Context, Result};
use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::fs;

/// Persistent server state under ~/.maplayer/
#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
    pub allowlist_path: PathBuf,
    pub key_path: PathBuf,
    pub profiles_root: PathBuf,
    pub config_path: PathBuf,
    pub local_token_path: PathBuf,
}

/// Authorized endpoint ids mapped to an optional client label.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Allowlist {
    pub endpoints: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    pub default_profile: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let root = dirs::home_dir().context("no home dir")?.join(".maplayer");
        Ok(Self {
            allowlist_path: root.join("allowlist.json"),
            key_path: root.join("secret_key"),
            profiles_root: root.join("profiles"),
            config_path: root.join("config.json"),
            local_token_path: root.join("local_token"),
            root,
        })
    }

    pub async fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.profiles_root).await?;
        Ok(())
    }

    pub async fn secret_key(&self) -> Result<SecretKey> {
        match fs::read(&self.key_path).await {
            Ok(bytes) if bytes.len() == 32 => Ok(SecretKey::from_bytes(
                &bytes.try_into().expect("len checked"),
            )),
            Ok(bytes) => String::from_utf8(bytes)?
                .trim()
                .parse()
                .context("corrupt secret_key file"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let key = SecretKey::generate();
                fs::write(&self.key_path, key.to_bytes()).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&self.key_path, std::fs::Permissions::from_mode(0o600))
                        .await?;
                }
                Ok(key)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn allowlist(&self) -> Result<Allowlist> {
        match fs::read(&self.allowlist_path).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Allowlist::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn allow(&self, id: &EndpointId, label: Option<String>) -> Result<()> {
        let mut list = self.allowlist().await.unwrap_or_default();
        list.endpoints.insert(
            id.to_string(),
            label.map(|l| l.replace('\n', " ").trim().to_string()),
        );
        fs::write(&self.allowlist_path, serde_json::to_vec_pretty(&list)?).await?;
        Ok(())
    }

    pub async fn is_allowed(&self, id: &EndpointId) -> bool {
        self.allowlist()
            .await
            .map(|l| l.endpoints.contains_key(&id.to_string()))
            .unwrap_or(false)
    }

    pub async fn server_config(&self) -> ServerConfig {
        fs::read(&self.config_path)
            .await
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub async fn save_server_config(&self, cfg: &ServerConfig) -> Result<()> {
        fs::write(&self.config_path, serde_json::to_vec_pretty(cfg)?).await?;
        Ok(())
    }

    /// Shared-secret token for same-host clients (the desktop launcher).
    /// Readable only by the local user; lets the launcher pair itself without
    /// consuming the one-shot pairing window reserved for remote devices.
    pub async fn local_token(&self) -> Result<String> {
        match fs::read_to_string(&self.local_token_path).await {
            Ok(t) => Ok(t.trim().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let token = uuid::Uuid::new_v4().simple().to_string();
                fs::write(&self.local_token_path, &token).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(
                        &self.local_token_path,
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .await?;
                }
                Ok(token)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn set_default_profile(&self, name: &str) -> Result<()> {
        let mut cfg = self.server_config().await;
        cfg.default_profile = Some(name.to_string());
        self.save_server_config(&cfg).await
    }
}
