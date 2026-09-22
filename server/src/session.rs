//! Managed agent sessions: each is an ACP-mode child process whose stdio we
//! multiplex to remote clients. Lines of ACP JSON-RPC are buffered for replay
//! so a client attaching late sees the backlog.

use crate::config::Config;
use crate::profiles::ProfileManager;
use crate::proto::{ManagedSession, SessionNewParams};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

const BACKLOG_CAP: usize = 4000;
const BROADCAST_CAP: usize = 512;

pub struct AcpSession {
    pub id: String,
    pub provider: String,
    pub profile: Option<String>,
    pub cwd: String,
    pub created_at: String,
    child: RwLock<Child>,
    stdin_tx: mpsc::Sender<Vec<u8>>,
    out_tx: broadcast::Sender<Vec<u8>>,
    backlog: RwLock<Vec<Vec<u8>>>,
    exited: RwLock<Option<i32>>,
}

impl AcpSession {
    pub fn pid(&self) -> Option<u32> {
        // Child is behind a lock; use try_read for status snapshots.
        self.child.try_read().ok().and_then(|c| c.id())
    }
}

#[derive(Default)]
pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<AcpSession>>>,
}

fn spawn_command(
    params: &SessionNewParams,
    codex_home: Option<std::path::PathBuf>,
) -> Result<Command> {
    let mut cmd = match params.provider.as_str() {
        "codex" => {
            // codex-acp adapter via npx; CODEX_HOME picks the account profile.
            let mut c = Command::new("npx");
            c.args(["-y", "@agentclientprotocol/codex-acp"]);
            if let Some(home) = codex_home {
                c.env("CODEX_HOME", home);
            }
            c
        }
        "cursor" => {
            let mut c = Command::new("agent");
            c.arg("acp");
            c
        }
        other => bail!("unknown provider: {other}"),
    };
    cmd.current_dir(&params.cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    Ok(cmd)
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn spawn(
        &self,
        params: SessionNewParams,
        cfg: &Config,
        profiles: &ProfileManager,
    ) -> Result<Arc<AcpSession>> {
        let codex_home = profiles.codex_home(params.profile.as_deref(), cfg).await?;
        let mut cmd = spawn_command(&params, codex_home)?;
        let mut child = cmd.spawn().context("spawn agent process")?;
        let stdin = child.stdin.take().context("child stdin")?;
        let stdout = child.stdout.take().context("child stdout")?;
        let stderr = child.stderr.take().context("child stderr")?;

        let id = Uuid::new_v4().to_string();
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, _) = broadcast::channel::<Vec<u8>>(BROADCAST_CAP);
        let session = Arc::new(AcpSession {
            id: id.clone(),
            provider: params.provider.clone(),
            profile: params.profile.clone(),
            cwd: params.cwd.clone(),
            created_at: chrono_now(),
            child: RwLock::new(child),
            stdin_tx,
            out_tx: out_tx.clone(),
            backlog: RwLock::new(Vec::new()),
            exited: RwLock::new(None),
        });

        // stdin writer
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = stdin_rx.recv().await {
                if stdin.write_all(&line).await.is_err() {
                    break;
                }
            }
        });

        // stdout reader → backlog + broadcast
        {
            let session = session.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut bytes = line.into_bytes();
                    bytes.push(b'\n');
                    let mut backlog = session.backlog.write().await;
                    if backlog.len() >= BACKLOG_CAP {
                        backlog.drain(..BACKLOG_CAP / 2);
                    }
                    backlog.push(bytes.clone());
                    drop(backlog);
                    let _ = out_tx.send(bytes);
                }
            });
        }

        // stderr → tracing
        let sid = id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(session = %sid, "agent stderr: {line}");
            }
        });

        // exit watcher
        {
            let session = session.clone();
            tokio::spawn(async move {
                let code = {
                    let mut child = session.child.write().await;
                    child.wait().await.ok().and_then(|s| s.code())
                };
                *session.exited.write().await = code;
            });
        }

        self.sessions.write().await.insert(id, session.clone());
        Ok(session)
    }

    pub async fn get(&self, id: &str) -> Option<Arc<AcpSession>> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn kill(&self, id: &str) -> Result<()> {
        let session = self.get(id).await.context("session not found")?;
        let mut child = session.child.write().await;
        child.kill().await?;
        Ok(())
    }

    pub async fn list(&self) -> Vec<ManagedSession> {
        let map = self.sessions.read().await;
        let mut out = Vec::new();
        for s in map.values() {
            out.push(ManagedSession {
                session_id: s.id.clone(),
                provider: s.provider.clone(),
                profile: s.profile.clone(),
                cwd: s.cwd.clone(),
                state: if s.exited.read().await.is_some() {
                    "exited"
                } else {
                    "running"
                }
                .into(),
                pid: s.pid(),
                created_at: s.created_at.clone(),
            });
        }
        out
    }

    /// Wire an attached client's ACP stream: replay backlog, then live lines;
    /// inbound lines go to child stdin.
    pub async fn bridge_acp(
        session: Arc<AcpSession>,
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
    ) -> Result<()> {
        let mut send = send;
        let mut recv = recv;

        // backlog replay
        for line in session.backlog.read().await.iter() {
            send.write_all(line).await?;
        }

        let mut rx = session.out_tx.subscribe();
        let stdin_tx = session.stdin_tx.clone();

        let mut send_task = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(bytes) => {
                        if send.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let mut recv_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            let mut pending = Vec::new();
            loop {
                match recv.read(&mut buf).await {
                    Ok(Some(n)) if n > 0 => {
                        pending.extend_from_slice(&buf[..n]);
                        while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
                            let line: Vec<u8> = pending.drain(..=pos).collect();
                            if line.is_empty() {
                                continue;
                            }
                            if stdin_tx.send(line).await.is_err() {
                                return;
                            }
                        }
                    }
                    _ => return,
                }
            }
        });

        tokio::select! {
            _ = &mut send_task => recv_task.abort(),
            _ = &mut recv_task => send_task.abort(),
        }
        Ok(())
    }
}

fn chrono_now() -> String {
    // Avoid pulling chrono; RFC3339-ish from SystemTime.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}
