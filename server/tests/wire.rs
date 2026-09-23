//! Wire-level integration test: a real `maplayer_server::Server` endpoint and
//! real iroh client endpoints talking over QUIC on localhost, with an
//! isolated ~/.maplayer root. Covers the whole control plane plus the ACP
//! stream bridge using MAPLAYER_STUB_AGENT.

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::Endpoint;
use maplayer_server::config::Config;
use maplayer_server::net::Server;
use maplayer_server::session::STUB_AGENT_ENV;
use maplayer_server::ALPN;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const PIN: &str = "123456";

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

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("maplayer-wire-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn new_endpoint() -> Endpoint {
    Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap()
}

/// Wait until the endpoint knows local addresses a peer can dial.
async fn wait_addrs(ep: &Endpoint) {
    for _ in 0..300 {
        if ep.addr().ip_addrs().next().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("endpoint never published direct addresses");
}

async fn read_line(recv: &mut RecvStream) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        match recv.read(&mut byte).await {
            Ok(Some(1)) => {
                if byte[0] == b'\n' {
                    return Ok(line);
                }
                line.push(byte[0]);
            }
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "eof",
                ))
            }
            Err(e) => return Err(std::io::Error::other(e.to_string())),
        }
    }
}

/// One control-plane round trip on a fresh rpc stream.
async fn rpc(conn: &Connection, method: &str, params: Value) -> Value {
    let (mut send, mut recv) = conn.open_bi().await.expect("open_bi");
    send.write_all(b"{\"kind\":\"rpc\",\"v\":1}\n")
        .await
        .unwrap();
    let req = serde_json::to_vec(&json!({
        "jsonrpc": "2.0", "id": 1, "method": method, "params": params,
    }))
    .unwrap();
    send.write_all(&req).await.unwrap();
    send.write_all(b"\n").await.unwrap();
    let _ = send.finish();
    let line = read_line(&mut recv).await.expect("rpc response line");
    serde_json::from_slice(&line).unwrap()
}

async fn rpc_ok(conn: &Connection, method: &str, params: Value) -> Value {
    let resp = rpc(conn, method, params).await;
    assert!(
        resp.get("error").is_none(),
        "{method} returned error: {resp}"
    );
    resp["result"].clone()
}

async fn rpc_err(conn: &Connection, method: &str, params: Value) -> Value {
    let resp = rpc(conn, method, params).await;
    resp.get("error")
        .unwrap_or_else(|| panic!("{method} unexpectedly succeeded: {resp}"))
        .clone()
}

/// Open an acp stream for `session_id`; caller writes/reads raw ACP lines.
async fn acp_attach(conn: &Connection, session_id: &str) -> (SendStream, RecvStream) {
    let (mut send, recv) = conn.open_bi().await.expect("acp open_bi");
    let header = serde_json::to_vec(&json!({
        "kind": "acp", "v": 1, "session_id": session_id,
    }))
    .unwrap();
    send.write_all(&header).await.unwrap();
    send.write_all(b"\n").await.unwrap();
    (send, recv)
}

/// A client that is not allowlisted connects fine but every method except
/// pair_hello answers -32001 (per-stream authorization).
async fn assert_unauthorized(addr: &iroh::EndpointAddr, tag: &str) -> (Endpoint, Connection) {
    let ep = new_endpoint().await;
    let conn = ep.connect(addr.clone(), ALPN).await.expect("connect");
    let e = rpc_err(&conn, "maplayer/ping", json!({})).await;
    assert_eq!(e["code"], -32001, "unauthorized ping accepted ({tag})");
    (ep, conn)
}

/// Read lines until `want` have arrived or the timeout hits.
async fn read_n_lines(recv: &mut RecvStream, want: usize, timeout: Duration) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let res = tokio::time::timeout(timeout, async {
        while lines.len() < want {
            match read_line(recv).await {
                Ok(l) => lines.push(l),
                Err(_) => break,
            }
        }
    })
    .await;
    let _ = res;
    lines
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wire_end_to_end() {
    let dir = tmpdir("srv");
    let cfg = test_config(&dir);
    let server = Arc::new(Server::bind(cfg).await.expect("server bind"));
    let server_id = server.endpoint_id().to_string();
    wait_addrs(&server.endpoint).await;
    let addr = server.endpoint.addr();

    {
        let server = server.clone();
        tokio::spawn(async move { server.run().await });
    }

    let client = new_endpoint().await;

    // --- Unauthorized outside a pairing window: methods refused ---
    let (_rogue_ep, rogue) = assert_unauthorized(&addr, "before pairing").await;
    // pair_hello with a bad PIN fails too.
    let e = rpc_err(&rogue, "maplayer/pair_hello", json!({"pin": "000000"})).await;
    assert!(e["message"].as_str().unwrap().contains("pin"), "{e}");

    // --- Pairing: bad PIN is a JSON-RPC error, good PIN pairs ---
    server.open_pairing(PIN.into());
    let conn = client.connect(addr.clone(), ALPN).await.expect("connect");
    let e = rpc_err(&conn, "maplayer/pair_hello", json!({"pin": "000000"})).await;
    assert!(e["message"].as_str().unwrap().contains("pin"), "{e}");
    let r = rpc_ok(
        &conn,
        "maplayer/pair_hello",
        json!({"pin": PIN, "label": "test-client"}),
    )
    .await;
    assert_eq!(r["server_id"], server_id);

    // The window closed on success: pair_hello now fails even for the paired
    // client, and a brand-new unauthorized endpoint is refused again.
    let e = rpc_err(&conn, "maplayer/pair_hello", json!({"pin": PIN})).await;
    assert!(e["message"].as_str().unwrap().contains("closed"), "{e}");
    let _closed_ep = assert_unauthorized(&addr, "after pairing closed").await;

    // The same-host launcher pairs via ~/.maplayer/local_token instead of the
    // PIN — works even with the pairing window closed.
    let token = std::fs::read_to_string(dir.join("local_token")).unwrap();
    let launcher = new_endpoint().await;
    let lconn = launcher.connect(addr.clone(), ALPN).await.expect("connect");
    let r = rpc_ok(
        &lconn,
        "maplayer/pair_hello",
        json!({"pin": token.trim(), "label": "launcher"}),
    )
    .await;
    assert_eq!(r["server_id"], server_id);
    let r = rpc_ok(&lconn, "maplayer/ping", json!({})).await;
    assert_eq!(r["server_id"], server_id);

    // --- ping ---
    let r = rpc_ok(&conn, "maplayer/ping", json!({})).await;
    assert_eq!(r["server_id"], server_id);
    assert!(r["version"].as_str().is_some());

    // --- unknown method: JSON-RPC standard -32601 ---
    let e = rpc_err(&conn, "maplayer/nope", json!({})).await;
    assert_eq!(e["code"], -32601);

    // --- sessions: empty, then one managed session ---
    let r = rpc_ok(&conn, "maplayer/sessions", json!({})).await;
    assert_eq!(r["managed"].as_array().unwrap().len(), 0);
    assert!(r["external"].is_array());

    // --- profiles: create both credential kinds, set default ---
    let r = rpc_ok(
        &conn,
        "maplayer/profile_new",
        json!({"name": "work", "credential": "chatgpt"}),
    )
    .await;
    assert_eq!(r["name"], "work");
    assert_eq!(r["login"]["kind"], "url");
    assert!(r["login"]["text"].as_str().unwrap().contains("codex login"));
    let r = rpc_ok(
        &conn,
        "maplayer/profile_new",
        json!({"name": "keys", "credential": "api-key"}),
    )
    .await;
    assert_eq!(r["login"]["kind"], "api_key_prompt");

    let r = rpc_ok(&conn, "maplayer/profiles", json!({})).await;
    let names: Vec<&str> = r["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"work") && names.contains(&"keys"),
        "{names:?}"
    );

    rpc_ok(&conn, "maplayer/profile_default", json!({"name": "work"})).await;
    let r = rpc_ok(&conn, "maplayer/profiles", json!({})).await;
    assert_eq!(r["default"], "work");

    // --- session_new: unknown provider, missing binary, then stub agent ---
    let e = rpc_err(
        &conn,
        "maplayer/session_new",
        json!({"provider": "not-a-provider", "cwd": dir.to_string_lossy()}),
    )
    .await;
    assert!(
        e["message"].as_str().unwrap().contains("unknown provider"),
        "{e}"
    );

    std::env::set_var(STUB_AGENT_ENV, "maplayer-no-such-binary-xyz");
    let e = rpc_err(
        &conn,
        "maplayer/session_new",
        json!({"provider": "codex", "cwd": dir.to_string_lossy()}),
    )
    .await;
    assert!(
        e["message"].as_str().unwrap().contains("missing binary"),
        "{e}"
    );

    std::env::set_var(STUB_AGENT_ENV, "cat");
    let r = rpc_ok(
        &conn,
        "maplayer/session_new",
        json!({"provider": "codex", "profile": "work", "cwd": dir.to_string_lossy()}),
    )
    .await;
    let session_id = r["session_id"].as_str().unwrap().to_string();

    let r = rpc_ok(&conn, "maplayer/sessions", json!({})).await;
    let managed = r["managed"].as_array().unwrap();
    assert_eq!(managed.len(), 1);
    assert_eq!(managed[0]["session_id"], session_id);
    assert_eq!(managed[0]["provider"], "codex");
    assert_eq!(managed[0]["state"], "running");

    // --- acp stream: stdin forwarding + live lines ---
    let (mut send1, mut recv1) = acp_attach(&conn, &session_id).await;
    send1.write_all(b"hello-acp\n").await.unwrap();
    let lines = read_n_lines(&mut recv1, 1, Duration::from_secs(10)).await;
    assert_eq!(lines, vec![b"hello-acp".to_vec()]);

    // Second attach: backlog replay first, in order.
    let (mut send2, mut recv2) = acp_attach(&conn, &session_id).await;
    let lines = read_n_lines(&mut recv2, 1, Duration::from_secs(10)).await;
    assert_eq!(lines, vec![b"hello-acp".to_vec()]);

    // Fan-out: client2 writes, both readers get the echo live.
    send2.write_all(b"live-two\n").await.unwrap();
    let lines1 = read_n_lines(&mut recv1, 1, Duration::from_secs(10)).await;
    let lines2 = read_n_lines(&mut recv2, 1, Duration::from_secs(10)).await;
    assert_eq!(lines1, vec![b"live-two".to_vec()]);
    assert_eq!(lines2, vec![b"live-two".to_vec()]);

    // --- unknown session id: clean error line, then close ---
    let (_s, mut bad_recv) = acp_attach(&conn, "not-a-session").await;
    let line = read_line(&mut bad_recv).await.expect("error line");
    let v: Value = serde_json::from_slice(&line).unwrap();
    assert_eq!(v["error"], "session not found");

    // --- session_tail: last lines of a codex rollout file ---
    let codex_dir = dir.join(".codex/sessions/2026/09");
    std::fs::create_dir_all(&codex_dir).unwrap();
    // tail_rollout is restricted to ~/.codex — point the test HOME at dir.
    let orig_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &dir);
    let roll = codex_dir.join("rollout-test.jsonl");
    std::fs::write(&roll, "l1\nl2\nl3\nl4\n").unwrap();
    let r = rpc_ok(
        &conn,
        "maplayer/session_tail",
        json!({"reference": roll.to_string_lossy(), "lines": 2}),
    )
    .await;
    assert_eq!(r["lines"], json!(["l3", "l4"]));
    let e = rpc_err(
        &conn,
        "maplayer/session_tail",
        json!({"reference": "/etc/passwd"}),
    )
    .await;
    assert!(e["message"].as_str().unwrap().contains("codex"), "{e}");
    match orig_home {
        Some(h) => std::env::set_var("HOME", h),
        None => std::env::remove_var("HOME"),
    }

    // --- session_kill ---
    rpc_ok(
        &conn,
        "maplayer/session_kill",
        json!({"session_id": session_id}),
    )
    .await;
    // State flips to exited; attached readers see EOF rather than a hang.
    let eof = read_n_lines(&mut recv1, 1, Duration::from_secs(10)).await;
    assert!(eof.is_empty(), "expected EOF after kill, got {eof:?}");

    let mut saw_exited = false;
    for _ in 0..200 {
        let r = rpc_ok(&conn, "maplayer/sessions", json!({})).await;
        if r["managed"][0]["state"] == "exited" {
            saw_exited = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(saw_exited, "session never reported exited");

    std::env::remove_var(STUB_AGENT_ENV);
    let _ = std::fs::remove_dir_all(&dir);
}
