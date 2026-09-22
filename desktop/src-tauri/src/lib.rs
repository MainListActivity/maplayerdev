mod client;
mod server_proc;

use client::Client;
use server_proc::{ServerProc, ServerStatus};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

struct AppState {
    proc: Arc<ServerProc>,
    client: Mutex<Option<CachedClient>>,
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
/// from the server's stdout (full pairing ticket while the window is open,
/// pin-less addr otherwise), then cache it keyed on that ticket.
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
        })
        .invoke_handler(tauri::generate_handler![
            start_server,
            stop_server,
            server_status,
            sessions,
            profiles,
        ])
        .run(tauri::generate_context!())
        .expect("error while running maplayer desktop");
}
