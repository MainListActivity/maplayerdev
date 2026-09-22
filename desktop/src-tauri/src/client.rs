//! Local iroh client: the desktop speaks to its own server through the same
//! wire protocol as Android — one protocol, two transports of reach.

use anyhow::{bail, Context, Result};
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

const ALPN: &[u8] = b"maplayer/1";

pub struct Client {
    endpoint: Endpoint,
    conn: Mutex<Option<Connection>>,
    next_id: AtomicU64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum StreamHeader {
    Rpc {
        v: u32,
    },
    #[allow(dead_code)]
    Acp {
        v: u32,
        session_id: String,
    },
}

/// The `{addr, pin}` ticket the server prints during `pair`. `pin` is
/// optional: the self-connect ticket the launcher builds from the scraped
/// `addr:` line carries none, which is fine once our endpoint id is already
/// on the server's allowlist.
#[derive(Debug, Serialize, Deserialize)]
struct Ticket {
    addr: EndpointAddr,
    #[serde(default)]
    pin: Option<String>,
}

impl Client {
    /// Load (or create) the desktop's persistent iroh identity under
    /// `~/.maplayer-desktop/` and bind an endpoint.
    pub async fn new() -> Result<Self> {
        let key_path = dirs::home_dir()
            .context("no home")?
            .join(".maplayer-desktop/secret_key");
        let secret = match tokio::fs::read(&key_path).await {
            Ok(b) if b.len() == 32 => SecretKey::from_bytes(&b.try_into().unwrap()),
            _ => {
                let k = SecretKey::generate();
                if let Some(parent) = key_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&key_path, k.to_bytes()).await?;
                k
            }
        };
        let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(secret)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;
        Ok(Self {
            endpoint,
            conn: Mutex::new(None),
            next_id: AtomicU64::new(0),
        })
    }

    /// Connect to the server described by a ticket. If our endpoint id is
    /// already on the server allowlist this is a plain connect. Otherwise —
    /// the server has no implicit trust for the local client — we present
    /// the ticket's pin via `maplayer/pair_hello` while the pairing window
    /// is open, then reconnect: the server captures `allowed` per connection
    /// at accept time, so the pre-pairing connection stays unauthorized.
    pub async fn connect(&self, ticket_json: &str) -> Result<()> {
        let ticket: Ticket = serde_json::from_str(ticket_json).context("bad ticket json")?;
        let conn = self
            .endpoint
            .connect(ticket.addr.clone(), ALPN)
            .await
            .context("connect to server")?;

        if self.ping(&conn).await.is_ok() {
            *self.conn.lock().await = Some(conn);
            return Ok(());
        }

        let pin = ticket
            .pin
            .filter(|p| !p.is_empty())
            .context("server rejected us and ticket carries no pin")?;
        rpc_on(
            &conn,
            &self.next_id,
            "maplayer/pair_hello",
            json!({ "pin": pin, "label": "maplayer-desktop" }),
        )
        .await
        .context("pair_hello failed")?;

        conn.close(0u8.into(), b"paired");
        let conn = self
            .endpoint
            .connect(ticket.addr, ALPN)
            .await
            .context("reconnect after pairing")?;
        self.ping(&conn).await.context("ping after pairing")?;
        *self.conn.lock().await = Some(conn);
        Ok(())
    }

    async fn ping(&self, conn: &Connection) -> Result<Value> {
        rpc_on(conn, &self.next_id, "maplayer/ping", json!({})).await
    }

    /// JSON-RPC over a fresh rpc stream.
    pub async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let conn = {
            let guard = self.conn.lock().await;
            guard.clone().context("not connected to server")?
        };
        rpc_on(&conn, &self.next_id, method, params).await
    }
}

/// One JSON-RPC request line after a `{kind:"rpc"}` stream header; reads the
/// single response line the server writes before finishing the stream.
async fn rpc_on(
    conn: &Connection,
    next_id: &AtomicU64,
    method: &str,
    params: Value,
) -> Result<Value> {
    let (mut send, mut recv) = conn.open_bi().await?;
    let header = serde_json::to_vec(&StreamHeader::Rpc { v: 1 })?;
    send.write_all(&header).await?;
    send.write_all(b"\n").await?;
    let id = next_id.fetch_add(1, Ordering::SeqCst);
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    send.write_all(&serde_json::to_vec(&req)?).await?;
    send.write_all(b"\n").await?;
    let _ = send.finish();

    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match recv.read(&mut byte).await {
            Ok(Some(1)) if byte[0] == b'\n' => break,
            Ok(Some(1)) => line.push(byte[0]),
            Ok(_) => bail!("eof"),
            Err(e) => bail!("read: {e}"),
        }
        if line.len() > (1 << 20) {
            bail!("response too long");
        }
    }
    let resp: Value = serde_json::from_slice(&line)?;
    if let Some(err) = resp.get("error") {
        bail!(
            "{}",
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("rpc error")
        );
    }
    Ok(resp["result"].clone())
}
