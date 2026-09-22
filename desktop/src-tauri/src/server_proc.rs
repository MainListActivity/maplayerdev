//! Owns the maplayer-server child process: spawn, scrape stdout for the
//! endpoint id / pairing PIN / ticket, relay log lines to the UI.

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
    std::env::var("MAPLAYER_SERVER_BIN").unwrap_or_else(|_| {
        // Dev layout: server binary built by `cargo build` at the workspace
        // root; packaged builds should set MAPLAYER_SERVER_BIN.
        for cand in [
            "../target/debug/maplayer-server",
            "../target/release/maplayer-server",
            "maplayer-server",
        ] {
            if std::path::Path::new(cand).exists() {
                return cand.to_string();
            }
        }
        "maplayer-server".to_string()
    })
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

    /// The ticket the in-process client uses to reach its own server —
    /// the same JSON a phone would paste, minus the pin.
    pub fn connected_ticket(&self) -> Option<String> {
        self.connected_ticket.lock().unwrap().clone()
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
                    // Build a self-connect ticket: {addr: <parsed>, pin: ""}.
                    if let Ok(addr) = serde_json::from_str::<serde_json::Value>(raw) {
                        *me.connected_ticket.lock().unwrap() =
                            Some(json!({ "addr": addr, "pin": "" }).to_string());
                    }
                }
                if let Some(rest) = line.strip_prefix("pairing ticket (QR payload):") {
                    *me.ticket.lock().unwrap() = Some(rest.trim().to_string());
                }
                let _ = app.emit("server-event", &line);
            }
            // Process exited — clear running state.
            *me.child.lock().unwrap() = None;
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
        Ok(())
    }
}
