//! Local iroh client: the desktop speaks to its own server through the same
//! wire protocol as Android — one protocol, two transports of reach.

use anyhow::{bail, Context, Result};
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

const ALPN: &[u8] = b"maplayer/1";

pub struct Client {
    endpoint: Endpoint,
    conn: Mutex<Option<iroh::endpoint::Connection>>,
    next_id: AtomicU64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum StreamHeader {
    Rpc { v: u32 },
    #[allow(dead_code)]
    Acp { v: u32, session_id: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct Ticket {
    addr: TicketAddr,
    #[allow(dead_code)]
    pin: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TicketAddr {
    id: String,
    #[serde(default)]
    addrs: Vec<Value>,
}

impl Ticket {
    fn endpoint_addr(&self) -> Result<EndpointAddr> {
        let id: EndpointId = self.addr.id.parse().context("bad endpoint id")?;
        let mut out = EndpointAddr::new(id);
        for a in &self.addr.addrs {
            if let Some(url) = a.get("Relay").and_then(|v| v.as_str()) {
                out = out.with_relay_url(url.parse()?);
            }
            if let Some(ip) = a.get("Ip").and_then(|v| v.as_str()) {
                out = out.with_ip_addr(ip.parse()?);
            }
        }
        Ok(out)
    }
}

impl Client {
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

    pub async fn connect(&self, ticket_json: &str) -> Result<()> {
        let ticket: Ticket = serde_json::from_str(ticket_json)?;
        let conn = self
            .endpoint
            .connect(ticket.endpoint_addr()?, ALPN)
            .await?;
        *self.conn.lock().await = Some(conn);
        Ok(())
    }

    /// JSON-RPC over a fresh rpc stream.
    pub async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let conn = {
            let guard = self.conn.lock().await;
            guard.clone().context("not connected to server")?
        };
        let (mut send, mut recv) = conn.open_bi().await?;
        let header = serde_json::to_vec(&StreamHeader::Rpc { v: 1 })?;
        send.write_all(&header).await?;
        send.write_all(b"\n").await?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
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
            bail!("{}", err.get("message").and_then(|m| m.as_str()).unwrap_or("rpc error"));
        }
        Ok(resp["result"].clone())
    }
}
