//! Wire types shared by the control plane and the ACP passthrough.
//! See server/proto/PROTOCOL.md for the on-wire contract.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StreamHeader {
    Rpc { v: u32 },
    Acp { v: u32, session_id: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairHelloParams {
    pub pin: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairHelloResult {
    pub server_id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionNewParams {
    pub provider: String,
    #[serde(default)]
    pub profile: Option<String>,
    pub cwd: String,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionNewResult {
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionKillParams {
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManagedSession {
    pub session_id: String,
    pub provider: String,
    pub profile: Option<String>,
    pub cwd: String,
    pub state: String,
    pub pid: Option<u32>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExternalSession {
    pub provider: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub title: Option<String>,
    pub last_active: Option<String>,
    pub alive: bool,
    pub detail: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionsResult {
    pub managed: Vec<ManagedSession>,
    pub external: Vec<ExternalSession>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub credential: String,
    pub codex_home: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfilesResult {
    pub profiles: Vec<Profile>,
    pub default: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileNewParams {
    pub name: String,
    pub credential: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginInstruction {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileNewResult {
    pub name: String,
    pub codex_home: String,
    pub login: LoginInstruction,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileDefaultParams {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PingResult {
    pub server_id: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairTicket {
    pub addr: iroh::EndpointAddr,
    pub pin: String,
}
