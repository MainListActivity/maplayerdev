//! Owns the maplayer-server child process: spawn, scrape stdout for the
//! endpoint id / addr / pairing ticket, relay log lines to the UI.

use anyhow::{bail, Context, Result};
use serde_json::json;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

#[derive(Default)]
pub struct ServerProc {
    child: Mutex<Option<Child>>,
    pairing: Mutex<bool>,
    endpoint_id: Mutex<Option<String>>,
    addr: Mutex<Option<String>>,
    ticket: Mutex<Option<String>>,
    connected_ticket: Mutex<Option<String>>,
}

#[derive(serde::Serialize, Clone)]
pub struct ServerStatus {
    pub running: bool,
    pub pairing: bool,
    pub endpoint_id: Option<String>,
    pub addr: Option<String>,
    pub ticket: Option<String>,
}

fn server_bin() -> String {
    if let Ok(v) = std::env::var("MAPLAYER_SERVER_BIN") {
        if !v.is_empty() {
            return v;
        }
    }
    let bin = format!("maplayer-server{}", std::env::consts::EXE_SUFFIX);
    let mut cands = Vec::new();
    // Workspace target dir, independent of launch CWD: src-tauri lives two
    // levels below the repo root, so ../../target is <repo>/target.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for profile in ["debug", "release"] {
        cands.push(manifest.join("../../target").join(profile).join(&bin));
    }
    // CWD-relative layouts: launched from the repo root, desktop/, or
    // desktop/src-tauri.
    for base in ["target", "../target", "../../target"] {
        for profile in ["debug", "release"] {
            cands.push(std::path::Path::new(base).join(profile).join(&bin));
        }
    }
    for cand in cands {
        if cand.is_file() {
            return cand.to_string_lossy().into_owned();
        }
    }
    bin // fall back to PATH lookup
}

impl ServerProc {
    pub fn status(&self) -> ServerStatus {
        ServerStatus {
            running: self.child.lock().unwrap().is_some(),
            pairing: *self.pairing.lock().unwrap(),
            endpoint_id: self.endpoint_id.lock().unwrap().clone(),
            addr: self.addr.lock().unwrap().clone(),
            ticket: self.ticket.lock().unwrap().clone(),
        }
    }

    /// The ticket the in-process client uses to reach its own server:
    /// `{addr, pin}` where pin is the launcher token from
    /// `~/.maplayer/local_token` — accepted by `pair_hello` without
    /// consuming the one-shot pairing window reserved for remote devices.
    pub fn connected_ticket(&self) -> Option<String> {
        self.connected_ticket.lock().unwrap().clone()
    }

    /// ~/.maplayer/local_token, written by the server at bind time.
    fn local_token() -> Option<String> {
        let path = dirs::home_dir()?.join(".maplayer/local_token");
        std::fs::read_to_string(path)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    pub async fn start(self: &Arc<Self>, app: &AppHandle, pair: bool) -> Result<()> {
        if self.child.lock().unwrap().is_some() {
            bail!("server already running");
        }
        let mut cmd = Command::new(server_bin());
        cmd.arg(if pair { "pair" } else { "serve" })
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("spawn maplayer-server")?;
        let stdout = child.stdout.take().context("server stdout")?;

        *self.pairing.lock().unwrap() = pair;
        *self.ticket.lock().unwrap() = None;
        *self.connected_ticket.lock().unwrap() = None;
        *self.child.lock().unwrap() = Some(child);

        let me = self.clone();
        let app = app.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = app.emit("server-log", &line);
                if let Some(rest) = line.strip_prefix("endpoint id:") {
                    *me.endpoint_id.lock().unwrap() = Some(rest.trim().to_string());
                }
                if let Some(rest) = line.strip_prefix("addr:") {
                    let raw = rest.trim();
                    *me.addr.lock().unwrap() = Some(raw.to_string());
                    // Self-connect ticket: pin is the launcher token, so the
                    // built-in client authorizes itself without touching the
                    // phone-facing pairing window.
                    if let Ok(addr) = serde_json::from_str::<serde_json::Value>(raw) {
                        *me.connected_ticket.lock().unwrap() = Some(
                            json!({ "addr": addr, "pin": Self::local_token() }).to_string(),
                        );
                    }
                }
                if let Some(rest) = line.strip_prefix("pairing ticket (QR payload):") {
                    let raw = rest.trim().to_string();
                    *me.ticket.lock().unwrap() = Some(raw.clone());
                    let _ = app.emit("pairing-ticket", raw);
                }
                let _ = app.emit("server-event", &line);
            }
            // Process exited — clear runtime state.
            *me.child.lock().unwrap() = None;
            *me.pairing.lock().unwrap() = false;
            *me.ticket.lock().unwrap() = None;
            *me.connected_ticket.lock().unwrap() = None;
            let _ = app.emit("server-event", "exited");
        });
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            child.kill().await?;
        }
        *self.pairing.lock().unwrap() = false;
        *self.ticket.lock().unwrap() = None;
        *self.connected_ticket.lock().unwrap() = None;
        Ok(())
    }
}
