//! Managed agent sessions: each is an ACP-mode child process whose stdio we
//! multiplex to remote clients. Lines of ACP JSON-RPC are buffered for replay
//! so a client attaching late sees the backlog.

use crate::config::Config;
use crate::profiles::ProfileManager;
use crate::proto::{ManagedSession, SessionNewParams};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, watch, RwLock};
use uuid::Uuid;

const BACKLOG_CAP: usize = 4000;
const BROADCAST_CAP: usize = 512;

/// Env override for tests: when set, `session_new` spawns this command
/// (program + whitespace-separated args) regardless of the requested provider.
pub const STUB_AGENT_ENV: &str = "MAPLAYER_STUB_AGENT";

/// One line of agent stdout, tagged with its stream position so attachers can
/// dedupe backlog replay against live broadcast.
type Line = (u64, Vec<u8>);

pub struct AcpSession {
    pub id: String,
    pub provider: String,
    pub profile: Option<String>,
    pub cwd: String,
    pub created_at: String,
    pid: Option<u32>,
    stdin_tx: mpsc::Sender<Vec<u8>>,
    /// Signal channel to the child-owner task; the child is owned outright
    /// there so `wait()` and `kill()` never contend on a lock.
    kill_tx: mpsc::Sender<()>,
    out_tx: broadcast::Sender<Line>,
    backlog: RwLock<Vec<Line>>,
    seq: AtomicU64,
    /// Exit status: `None` while running, `Some(code)` once reaped. Signal
    /// kills carry no code — recorded as -1 so "exited" is still observable.
    exit_tx: watch::Sender<Option<i32>>,
    /// `true` once stdout hit EOF — no more agent output will ever arrive.
    done_tx: watch::Sender<bool>,
}

impl AcpSession {
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn exited(&self) -> bool {
        self.exit_tx.borrow().is_some()
    }

    /// SIGKILL the agent and wait for its exit to be recorded.
    pub async fn kill(&self) {
        if self.exited() {
            return;
        }
        let _ = self.kill_tx.send(()).await;
        let mut rx = self.exit_tx.subscribe();
        while rx.borrow().is_none() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    }
}

#[derive(Default)]
pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<AcpSession>>>,
}

/// What the spawn would run, for error messages and the stub override.
fn build_command(
    params: &SessionNewParams,
    codex_home: Option<std::path::PathBuf>,
) -> Result<Command> {
    let mut cmd = if let Ok(stub) = std::env::var(STUB_AGENT_ENV) {
        let mut it = stub.split_whitespace();
        let prog = it.next().context(format!("{STUB_AGENT_ENV} is empty"))?;
        let mut c = Command::new(prog);
        c.args(it);
        c
    } else {
        match params.provider.as_str() {
            "codex" => {
                // codex-acp adapter via npx; CODEX_HOME picks the account profile.
                let mut c = Command::new("npx");
                c.args(["-y", "@agentclientprotocol/codex-acp"]);
                c
            }
            "cursor" => {
                let mut c = Command::new("agent");
                c.arg("acp");
                c
            }
            other => bail!("unknown provider: {other}"),
        }
    };
    if let Some(home) = codex_home {
        cmd.env("CODEX_HOME", home);
    }
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
        let mut cmd = build_command(&params, codex_home)?;
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "spawn {} agent for provider '{}' failed (missing binary?)",
                std::env::var(STUB_AGENT_ENV)
                    .map(|s| format!("stub '{s}'"))
                    .unwrap_or_else(|_| "ACP".into()),
                params.provider
            )
        })?;
        let pid = child.id();
        let stdin = child.stdin.take().context("child stdin")?;
        let stdout = child.stdout.take().context("child stdout")?;
        let stderr = child.stderr.take().context("child stderr")?;

        let id = Uuid::new_v4().to_string();
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(64);
        let (out_tx, _) = broadcast::channel::<Line>(BROADCAST_CAP);
        let (done_tx, _) = watch::channel(false);
        let (kill_tx, mut kill_rx) = mpsc::channel::<()>(4);
        let (exit_tx, _) = watch::channel(None);
        let session = Arc::new(AcpSession {
            id: id.clone(),
            provider: params.provider.clone(),
            profile: params.profile.clone(),
            cwd: params.cwd.clone(),
            created_at: unix_now(),
            pid,
            stdin_tx,
            kill_tx,
            out_tx: out_tx.clone(),
            backlog: RwLock::new(Vec::new()),
            seq: AtomicU64::new(0),
            exit_tx: exit_tx.clone(),
            done_tx,
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

        // stdout reader → seq-tagged backlog + broadcast; EOF marks the
        // session done so bridges stop waiting for more output.
        {
            let session = session.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut bytes = line.into_bytes();
                    bytes.push(b'\n');
                    let seq = session.seq.fetch_add(1, Ordering::SeqCst);
                    {
                        let mut backlog = session.backlog.write().await;
                        if backlog.len() >= BACKLOG_CAP {
                            backlog.drain(..BACKLOG_CAP / 2);
                        }
                        backlog.push((seq, bytes.clone()));
                    }
                    let _ = out_tx.send((seq, bytes));
                }
                let _ = session.done_tx.send(true);
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

        // child owner: waits for exit, answers kill signals. Sole owner of
        // the Child handle — no lock can deadlock wait() against kill().
        tokio::spawn(async move {
            let mut child = child;
            let status = tokio::select! {
                status = child.wait() => status.ok(),
                _ = kill_rx.recv() => {
                    let _ = child.start_kill();
                    child.wait().await.ok()
                }
            };
            let _ = exit_tx.send(Some(status.and_then(|s| s.code()).unwrap_or(-1)));
        });

        self.sessions.write().await.insert(id, session.clone());
        Ok(session)
    }

    pub async fn get(&self, id: &str) -> Option<Arc<AcpSession>> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn kill(&self, id: &str) -> Result<()> {
        let session = self.get(id).await.context("session not found")?;
        session.kill().await;
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
                state: if s.exited() { "exited" } else { "running" }.into(),
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
        Self::bridge_io(session, send, recv).await
    }

    /// Same as `bridge_acp` over any byte stream pair — the seam unit tests
    /// attach to without an iroh connection.
    pub async fn bridge_io<W, R>(session: Arc<AcpSession>, mut send: W, mut recv: R) -> Result<()>
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        // Subscribe before snapshotting the backlog; live lines that arrive
        // mid-replay land in the receiver and are deduped by seq below.
        let mut rx = session.out_tx.subscribe();
        let (backlog, last_seq) = {
            let b = session.backlog.read().await;
            (b.clone(), b.last().map(|l| l.0))
        };
        for (_, line) in &backlog {
            send.write_all(line).await?;
        }

        let stdin_tx = session.stdin_tx.clone();
        let mut done = session.done_tx.subscribe();

        // Attached after the agent exited: backlog was replayed, nothing more
        // will come — close cleanly instead of hanging on recv().
        if *done.borrow() {
            let _ = send.shutdown().await;
            return Ok(());
        }

        let mut send_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    r = rx.recv() => match r {
                        Ok((seq, bytes)) => {
                            if Some(seq) <= last_seq {
                                continue;
                            }
                            if send.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    _ = done.changed() => {
                        // Agent stdout EOF: flush what was broadcast before
                        // the EOF, then close.
                        while let Ok((seq, bytes)) = rx.try_recv() {
                            if Some(seq) > last_seq && send.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        break;
                    }
                }
            }
            let _ = send.shutdown().await;
        });

        let mut recv_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 65536];
            let mut pending = Vec::new();
            loop {
                match recv.read(&mut buf).await {
                    Ok(0) => return,
                    Ok(n) => {
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
                    Err(_) => return,
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

fn unix_now() -> String {
    // Avoid pulling chrono; RFC3339-ish from SystemTime.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;

    // Tests mutate MAPLAYER_STUB_AGENT; serialize them.
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn test_config(dir: &Path) -> Config {
        Config {
            root: dir.into(),
            allowlist_path: dir.join("allowlist.json"),
            key_path: dir.join("secret_key"),
            profiles_root: dir.join("profiles"),
            config_path: dir.join("config.json"),
            local_token_path: dir.join("local_token"),
        }
    }

    fn params(provider: &str, cwd: &Path) -> SessionNewParams {
        SessionNewParams {
            provider: provider.into(),
            profile: None,
            cwd: cwd.to_string_lossy().into(),
        }
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("maplayer-session-test-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn wait_backlog(session: &AcpSession, n: usize) {
        for _ in 0..200 {
            if session.backlog.read().await.len() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {n} backlog lines");
    }

    #[tokio::test]
    async fn stub_echo_session_backlog_fanout_stdin_kill() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var(STUB_AGENT_ENV, "cat");

        let dir = tmpdir("echo");
        let cfg = test_config(&dir);
        cfg.ensure_dirs().await.unwrap();
        let profiles = ProfileManager::new(&cfg);
        let mgr = SessionManager::new();

        let session = mgr
            .spawn(params("codex", &dir), &cfg, &profiles)
            .await
            .expect("spawn cat stub");

        // stdin forwarding: line in → cat echoes → backlog grows, in order.
        session.stdin_tx.send(b"one\n".to_vec()).await.unwrap();
        session.stdin_tx.send(b"two\n".to_vec()).await.unwrap();
        wait_backlog(&session, 2).await;
        {
            let b = session.backlog.read().await;
            assert_eq!(b[0].1, b"one\n");
            assert_eq!(b[1].1, b"two\n");
            assert!(b[0].0 < b[1].0, "seq must be increasing");
        }

        // Fan-out: two subscribers both see a later live line.
        let mut rx1 = session.out_tx.subscribe();
        let mut rx2 = session.out_tx.subscribe();
        session.stdin_tx.send(b"three\n".to_vec()).await.unwrap();
        let (s1, l1) = rx1.recv().await.unwrap();
        let (s2, l2) = rx2.recv().await.unwrap();
        assert_eq!(l1, b"three\n");
        assert_eq!(l1, l2);
        assert_eq!(s1, s2);

        // Backlog replay order through the real bridge: attach a duplex
        // client, expect "one","two","three" in order, then a live echo of
        // a client-written line (stdin forwarding end to end).
        let (mut client_send, server_recv) = tokio::io::duplex(64);
        let (server_send, mut client_recv) = tokio::io::duplex(65536);
        let s2 = session.clone();
        let bridge =
            tokio::spawn(
                async move { SessionManager::bridge_io(s2, server_send, server_recv).await },
            );

        use tokio::io::AsyncReadExt;
        client_send.write_all(b"four\n").await.unwrap();
        let mut got = Vec::new();
        let mut buf = vec![0u8; 4096];
        tokio::time::timeout(Duration::from_secs(5), async {
            while got != b"one\ntwo\nthree\nfour\n" {
                let n = client_recv.read(&mut buf).await.unwrap();
                assert!(n > 0, "bridge closed early; got {got:?}");
                got.extend_from_slice(&buf[..n]);
            }
        })
        .await
        .expect("bridge read timeout");
        assert_eq!(got, b"one\ntwo\nthree\nfour\n");

        // Kill: child dies, exited is recorded, list() reports "exited".
        drop(client_send);
        drop(bridge);
        mgr.kill(&session.id).await.unwrap();
        for _ in 0..200 {
            if session.exited() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(session.exited());
        let listed = mgr.list().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, "exited");

        std::env::remove_var(STUB_AGENT_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn unknown_provider_errors() {
        let _guard = ENV_LOCK.lock().await;
        std::env::remove_var(STUB_AGENT_ENV);
        let dir = tmpdir("noprovider");
        let cfg = test_config(&dir);
        cfg.ensure_dirs().await.unwrap();
        let profiles = ProfileManager::new(&cfg);
        let mgr = SessionManager::new();
        let err = mgr
            .spawn(params("not-a-provider", &dir), &cfg, &profiles)
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("unknown provider"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn missing_stub_binary_errors() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var(STUB_AGENT_ENV, "maplayer-no-such-binary-xyz");
        let dir = tmpdir("nobin");
        let cfg = test_config(&dir);
        cfg.ensure_dirs().await.unwrap();
        let profiles = ProfileManager::new(&cfg);
        let mgr = SessionManager::new();
        let err = mgr
            .spawn(params("codex", &dir), &cfg, &profiles)
            .await
            .err()
            .unwrap();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("missing binary") || msg.contains("spawn"),
            "got: {msg}"
        );
        std::env::remove_var(STUB_AGENT_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
