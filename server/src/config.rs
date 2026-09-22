use anyhow::{Context, Result};
use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use tokio::fs;

/// Persistent server state under ~/.maplayer/
pub struct Config {
    pub root: PathBuf,
    pub allowlist_path: PathBuf,
    pub key_path: PathBuf,
    pub profiles_root: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Allowlist {
    pub nodes: BTreeSet<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    pub default_profile: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let root = dirs::home_dir()
            .context("no home dir")?
            .join(".maplayer");
        Ok(Self {
            allowlist_path: root.join("allowlist.json"),
            key_path: root.join("secret_key"),
            profiles_root: root.join("profiles"),
            config_path: root.join("config.json"),
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
        list.nodes.insert(id.to_string());
        if let Some(label) = label {
            list.nodes.insert(format!("{id}#{}", label.replace('\n', " ")));
        }
        fs::write(&self.allowlist_path, serde_json::to_vec_pretty(&list)?).await?;
        Ok(())
    }

    pub async fn is_allowed(&self, id: &EndpointId) -> bool {
        self.allowlist()
            .await
            .map(|l| l.nodes.iter().any(|n| n.split('#').next() == Some(&id.to_string())))
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
}
