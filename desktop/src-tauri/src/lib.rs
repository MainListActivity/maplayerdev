mod client;
mod server_proc;

use client::Client;
use server_proc::{ServerProc, ServerStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Mutex};

struct AppState {
    proc: Arc<ServerProc>,
    client: Mutex<Option<CachedClient>>,
    /// Open ACP bridges: session_id -> line writer feeding the agent's stdin.
    acp: Mutex<HashMap<String, mpsc::UnboundedSender<String>>>,
}

struct CachedClient {
    ticket: String,
    client: Arc<Client>,
}

type S<'a> = State<'a, AppState>;

#[tauri::command]
async fn start_server(app: AppHandle, state: S<'_>, pair: bool) -> Result<(), String> {
    state
        .proc
        .start(&app, pair)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn stop_server(state: S<'_>) -> Result<(), String> {
    state.proc.stop().await.map_err(|e| e.to_string())?;
    *state.client.lock().await = None;
    Ok(())
}

#[tauri::command]
fn server_status(state: S<'_>) -> ServerStatus {
    state.proc.status()
}

/// Lazily connect the built-in client via the ticket ServerProc scraped
/// from the server's stdout (addr + local launcher token), then cache it
/// keyed on that ticket.
async fn ensure_client(state: &S<'_>) -> Result<Arc<Client>, String> {
    let ticket = state
        .proc
        .connected_ticket()
        .ok_or("server not up / addr unknown")?;
    let mut guard = state.client.lock().await;
    if let Some(c) = guard.as_ref() {
        if c.ticket == ticket {
            return Ok(c.client.clone());
        }
    }
    let client = Client::new().await.map_err(|e| e.to_string())?;
    client.connect(&ticket).await.map_err(|e| e.to_string())?;
    let client = Arc::new(client);
    *guard = Some(CachedClient {
        ticket,
        client: client.clone(),
    });
    Ok(client)
}

/// Control-plane call with one reconnect-and-retry: a dead cached
/// connection (server restarted, addr changed) should not surface as an
/// error before we've tried a fresh connect.
async fn control(state: &S<'_>, method: &str) -> Result<serde_json::Value, String> {
    let client = ensure_client(state).await?;
    match client.rpc(method, serde_json::json!({})).await {
        Ok(v) => Ok(v),
        Err(_) => {
            *state.client.lock().await = None;
            let client = ensure_client(state).await?;
            client
                .rpc(method, serde_json::json!({}))
                .await
                .map_err(|e| e.to_string())
        }
    }
}

#[tauri::command]
async fn sessions(state: S<'_>) -> Result<serde_json::Value, String> {
    control(&state, "maplayer/sessions").await
}

#[tauri::command]
async fn profiles(state: S<'_>) -> Result<serde_json::Value, String> {
    control(&state, "maplayer/profiles").await
}

#[tauri::command]
async fn session_kill(state: S<'_>, session_id: String) -> Result<(), String> {
    let client = ensure_client(&state).await?;
    client
        .rpc(
            "maplayer/session_kill",
            serde_json::json!({ "session_id": session_id }),
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn session_tail(
    state: S<'_>,
    reference: String,
    lines: Option<u32>,
) -> Result<serde_json::Value, String> {
    let client = ensure_client(&state).await?;
    client
        .rpc(
            "maplayer/session_tail",
            serde_json::json!({ "reference": reference, "lines": lines }),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Attach to a managed session's ACP stream: replays the backlog, then
/// emits `acp-line` events ({session_id, line}) until EOF.
#[tauri::command]
async fn session_open(app: AppHandle, state: S<'_>, session_id: String) -> Result<(), String> {
    let client = ensure_client(&state).await?;
    let (mut send, mut recv) = client
        .open_acp(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    state.acp.lock().await.insert(session_id.clone(), tx);

    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if send.write_all(line.as_bytes()).await.is_err()
                || send.write_all(b"\n").await.is_err()
            {
                break;
            }
        }
    });

    let sid = session_id;
    tokio::spawn(async move {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match recv.read(&mut byte).await {
                Ok(Some(1)) => {
                    if byte[0] == b'\n' {
                        let text = String::from_utf8_lossy(&line).into_owned();
                        let _ = app.emit(
                            "acp-line",
                            serde_json::json!({ "session_id": sid, "line": text }),
                        );
                        line.clear();
                    } else {
                        line.push(byte[0]);
                    }
                    if line.len() > (1 << 20) {
                        break;
                    }
                }
                _ => break,
            }
        }
        let _ = app.emit(
            "acp-line",
            serde_json::json!({ "session_id": sid, "eof": true }),
        );
    });
    Ok(())
}

/// Write one JSON-RPC line onto an open ACP stream.
#[tauri::command]
async fn session_send(state: S<'_>, session_id: String, line: String) -> Result<(), String> {
    let guard = state.acp.lock().await;
    guard
        .get(&session_id)
        .ok_or("session not open")?
        .send(line)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn session_close(state: S<'_>, session_id: String) -> Result<(), String> {
    state.acp.lock().await.remove(&session_id);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tauri::Builder::default()
        .manage(AppState {
            proc: Arc::new(ServerProc::default()),
            client: Mutex::new(None),
            acp: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            start_server,
            stop_server,
            server_status,
            sessions,
            profiles,
            session_kill,
            session_tail,
            session_open,
            session_send,
            session_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running maplayer desktop");
}
