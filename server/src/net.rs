//! iroh transport: endpoint bring-up, pairing window, allowlist gate, and
//! per-stream dispatch to either the control-plane RPC handler or an ACP
//! session bridge.

use crate::config::Config;
use crate::discovery;
use crate::profiles::ProfileManager;
use crate::proto::*;
use crate::session::SessionManager;
use crate::ALPN;
use anyhow::{bail, Context, Result};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointId};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const MAX_LINE: usize = 1 << 20;

#[derive(Clone)]
pub struct Server {
    pub endpoint: Endpoint,
    pub config: Config,
    pub sessions: Arc<SessionManager>,
    pub profiles: Arc<ProfileManager>,
    pairing: Arc<PairingState>,
}

struct PairingState {
    open: AtomicBool,
    pin: std::sync::Mutex<Option<String>>,
}

impl PairingState {
    fn check(&self, pin: &str) -> bool {
        let guard = self.pin.lock().unwrap();
        self.open.load(Ordering::SeqCst) && guard.as_deref() == Some(pin)
    }
    /// The window admits exactly one client: first successful pair closes it.
    fn close(&self) {
        self.open.store(false, Ordering::SeqCst);
        *self.pin.lock().unwrap() = None;
    }
}

impl Server {
    pub async fn bind(cfg: Config) -> Result<Self> {
        cfg.ensure_dirs().await?;
        let secret = cfg.secret_key().await?;
        let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(secret)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .context("bind iroh endpoint")?;
        // Materialize the launcher token so same-host clients can read it
        // as soon as the endpoint is up.
        cfg.local_token().await?;
        Ok(Self {
            endpoint,
            profiles: Arc::new(ProfileManager::new(&cfg)),
            sessions: Arc::new(SessionManager::new()),
            config: cfg,
            pairing: Arc::new(PairingState {
                open: AtomicBool::new(false),
                pin: std::sync::Mutex::new(None),
            }),
        })
    }

    pub fn endpoint_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Open a pairing window: clients presenting `pin` join the allowlist.
    pub fn open_pairing(&self, pin: String) {
        *self.pairing.pin.lock().unwrap() = Some(pin);
        self.pairing.open.store(true, Ordering::SeqCst);
    }

    /// Ticket a client can render as QR: endpoint addr + pairing pin.
    pub async fn pair_ticket(&self) -> Result<PairTicket> {
        let pin = self
            .pairing
            .pin
            .lock()
            .unwrap()
            .clone()
            .context("pairing not open")?;
        Ok(PairTicket {
            addr: self.endpoint.addr(),
            pin,
        })
    }

    pub async fn run(&self) -> Result<()> {
        tracing::info!(id = %self.endpoint_id(), "maplayer-server listening");
        while let Some(incoming) = self.endpoint.accept().await {
            let accepting = match incoming.accept() {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!("reject incoming: {e}");
                    continue;
                }
            };
            let server = self.clone();
            tokio::spawn(async move {
                match accepting.await {
                    Ok(conn) => server.handle_conn(conn).await,
                    Err(e) => tracing::warn!("connection failed: {e}"),
                }
            });
        }
        Ok(())
    }

    async fn handle_conn(&self, conn: Connection) {
        // No connection-level gate: unauthorized endpoints may reach stream
        // dispatch, where every method except pair_hello returns -32001.
        // pair_hello itself is gated by the pairing PIN or the local
        // launcher token, so this is not an auth bypass — it exists so the
        // same-host launcher can pair against a `serve` daemon whose window
        // never opens.
        let remote = conn.remote_id();
        loop {
            match conn.accept_bi().await {
                Ok((send, recv)) => {
                    let server = self.clone();
                    tokio::spawn(async move {
                        // Re-check per stream: a client that paired on this
                        // connection is authorized from its next stream on.
                        let allowed_now = server.config.is_allowed(&remote).await;
                        if let Err(e) = server.handle_stream(remote, allowed_now, send, recv).await
                        {
                            tracing::debug!(%remote, "stream closed: {e}");
                        }
                    });
                }
                Err(_) => return,
            }
        }
    }

    async fn handle_stream(
        &self,
        remote: EndpointId,
        allowed: bool,
        mut send: SendStream,
        mut recv: RecvStream,
    ) -> Result<()> {
        let header = read_line(&mut recv).await?;
        let header: StreamHeader = serde_json::from_slice(&header)?;
        match header {
            StreamHeader::Rpc { .. } => {
                let req = read_line(&mut recv).await?;
                let req: Value = serde_json::from_slice(&req)?;
                let resp = self.dispatch(remote, allowed, req).await;
                send.write_all(&serde_json::to_vec(&resp)?).await?;
                send.write_all(b"\n").await?;
                let _ = send.finish();
            }
            StreamHeader::Acp { session_id, .. } => {
                if !allowed {
                    bail!("unauthorized acp attach");
                }
                match self.sessions.get(&session_id).await {
                    Some(session) => {
                        SessionManager::bridge_acp(session, send, recv).await?;
                    }
                    None => {
                        let line = serde_json::to_vec(&json!({
                            "error": "session not found",
                            "session_id": session_id,
                        }))?;
                        let _ = send.write_all(&line).await;
                        let _ = send.write_all(b"\n").await;
                        let _ = send.finish();
                    }
                }
            }
        }
        Ok(())
    }

    async fn dispatch(&self, remote: EndpointId, allowed: bool, req: Value) -> Value {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));

        // Pre-authorization surface: pair_hello only.
        if !allowed && method != "maplayer/pair_hello" {
            return err(id, -32001, "unauthorized");
        }
        const KNOWN: &[&str] = &[
            "maplayer/ping",
            "maplayer/pair_hello",
            "maplayer/sessions",
            "maplayer/session_new",
            "maplayer/session_kill",
            "maplayer/session_tail",
            "maplayer/profiles",
            "maplayer/profile_new",
            "maplayer/profile_default",
        ];
        if !KNOWN.contains(&method) {
            return err(id, -32601, "method not found");
        }

        match self.dispatch_result(remote, method, params).await {
            Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
            Err(e) => err(id, -32000, &format!("{e:#}")),
        }
    }

    async fn dispatch_result(
        &self,
        remote: EndpointId,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        match method {
            "maplayer/ping" => Ok(json!({
                "server_id": self.endpoint_id().to_string(),
                "version": env!("CARGO_PKG_VERSION"),
            })),
            "maplayer/pair_hello" => self.pair_hello(remote, params).await,
            "maplayer/sessions" => {
                let managed = self.sessions.list().await;
                let external = discovery::external_sessions();
                Ok(serde_json::to_value(SessionsResult { managed, external })?)
            }
            "maplayer/session_new" => {
                let p: SessionNewParams = serde_json::from_value(params)?;
                self.sessions
                    .spawn(p, &self.config, &self.profiles)
                    .await
                    .map(|s| json!({ "session_id": s.id }))
            }
            "maplayer/session_kill" => {
                let p: SessionKillParams = serde_json::from_value(params)?;
                self.sessions.kill(&p.session_id).await.map(|_| json!({}))
            }
            "maplayer/session_tail" => {
                let p: SessionTailParams = serde_json::from_value(params)?;
                let (lines, offset) =
                    discovery::tail_rollout(&p.reference, p.lines.unwrap_or(50).min(500))?;
                Ok(serde_json::to_value(SessionTailResult { lines, offset })?)
            }
            "maplayer/profiles" => {
                let profiles = self.profiles.list().await?;
                let default = self.config.server_config().await.default_profile;
                Ok(serde_json::to_value(ProfilesResult { profiles, default })?)
            }
            "maplayer/profile_new" => {
                let p: ProfileNewParams = serde_json::from_value(params)?;
                let r = self.profiles.create(&p.name, &p.credential).await?;
                Ok(serde_json::to_value(r)?)
            }
            "maplayer/profile_default" => {
                let p: ProfileDefaultParams = serde_json::from_value(params)?;
                self.profiles
                    .codex_home(Some(&p.name), &self.config)
                    .await?;
                self.config.set_default_profile(&p.name).await?;
                Ok(json!({}))
            }
            _ => unreachable!("method list checked in dispatch"),
        }
    }

    async fn pair_hello(&self, remote: EndpointId, params: Value) -> Result<Value> {
        let p: PairHelloParams = serde_json::from_value(params)?;
        if !self.pairing.check(&p.pin) {
            // Same-host launcher authenticates with the local token instead
            // of the pairing PIN, so it never consumes the one-shot window.
            self.check_local_token(&p.pin).await?;
        } else {
            // One window, one client: close so the PIN can't be reused.
            self.pairing.close();
        }
        self.config.allow(&remote, p.label).await?;
        tracing::info!(%remote, "paired new client");
        Ok(serde_json::to_value(PairHelloResult {
            server_id: self.endpoint_id().to_string(),
            name: "maplayer-server".into(),
        })?)
    }

    /// Constant-time-ish check of the local launcher token stored under
    /// ~/.maplayer/local_token (0600, same user only).
    async fn check_local_token(&self, pin: &str) -> Result<()> {
        let token = self.config.local_token().await?;
        if token.is_empty() || token != pin {
            bail!("pairing closed or bad pin");
        }
        Ok(())
    }
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn read_line(recv: &mut RecvStream) -> Result<Vec<u8>> {
    let mut line = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        if line.len() > MAX_LINE {
            bail!("line too long");
        }
        match recv.read(&mut byte).await {
            Ok(Some(1)) => {
                if byte[0] == b'\n' {
                    return Ok(line);
                }
                line.push(byte[0]);
            }
            Ok(_) => bail!("eof before newline"),
            Err(e) => bail!("read error: {e}"),
        }
    }
}
