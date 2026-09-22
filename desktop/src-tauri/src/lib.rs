mod client;
mod server_proc;

use client::Client;
use server_proc::{ServerProc, ServerStatus};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

struct AppState {
    proc: Arc<ServerProc>,
    client: Mutex<Option<Arc<Client>>>,
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

async fn ensure_client(state: &S<'_>) -> Result<Arc<Client>, String> {
    let mut guard = state.client.lock().await;
    if let Some(c) = guard.as_ref() {
        return Ok(c.clone());
    }
    let ticket = state
        .proc
        .connected_ticket()
        .ok_or("server not up / addr unknown")?;
    let client = Client::new().await.map_err(|e| e.to_string())?;
    client.connect(&ticket).await.map_err(|e| e.to_string())?;
    let client = Arc::new(client);
    *guard = Some(client.clone());
    Ok(client)
}

#[tauri::command]
async fn sessions(state: S<'_>) -> Result<serde_json::Value, String> {
    let c = ensure_client(&state).await?;
    c.rpc("maplayer/sessions", serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn profiles(state: S<'_>) -> Result<serde_json::Value, String> {
    let c = ensure_client(&state).await?;
    c.rpc("maplayer/profiles", serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())
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
