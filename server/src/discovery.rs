//! Read-only discovery of agent sessions the user started outside maplayer.
//! Codex: ~/.codex/sessions/**/rollout-*.jsonl (tail-capable).
//! Cursor: ~/.cursor/chats/<project>/<chat>/store.db (list-level only).

use crate::proto::ExternalSession;
use std::path::PathBuf;

const CODEX_LIMIT: usize = 50;
const CURSOR_LIMIT: usize = 50;

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

fn mtime_secs(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn pgrep(pattern: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn walk(dir: &PathBuf, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                walk(&p, depth - 1, out);
            } else {
                out.push(p);
            }
        }
    }
}

pub fn codex_sessions() -> Vec<ExternalSession> {
    let mut files = Vec::new();
    if let Some(root) = home().map(|h| h.join(".codex/sessions")) {
        walk(&root, 4, &mut files);
    }
    files.retain(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().starts_with("rollout-"))
            .unwrap_or(false)
            && p.extension().map(|e| e == "jsonl").unwrap_or(false)
    });
    files.sort_by_key(|p| std::cmp::Reverse(mtime_secs(p).unwrap_or(0)));
    files.truncate(CODEX_LIMIT);
    let running = pgrep("codex");
    files
        .into_iter()
        .map(|p| ExternalSession {
            provider: "codex".into(),
            reference: p.to_string_lossy().into(),
            title: p.file_name().map(|n| n.to_string_lossy().into()),
            last_active: mtime_secs(&p).map(|s| s.to_string()),
            alive: running,
            detail: "rollout jsonl; tail-capable".into(),
        })
        .collect()
}

pub fn cursor_sessions() -> Vec<ExternalSession> {
    let mut dbs = Vec::new();
    if let Some(root) = home().map(|h| h.join(".cursor/chats")) {
        walk(&root, 3, &mut dbs);
    }
    dbs.retain(|p| p.file_name().map(|n| n == "store.db").unwrap_or(false));
    dbs.sort_by_key(|p| std::cmp::Reverse(mtime_secs(p).unwrap_or(0)));
    dbs.truncate(CURSOR_LIMIT);
    let running = pgrep("cursor-agent|agent acp");
    dbs.into_iter()
        .map(|p| {
            let chat_id = p
                .parent()
                .and_then(|d| d.file_name())
                .map(|n| n.to_string_lossy().into());
            ExternalSession {
                provider: "cursor".into(),
                reference: p.to_string_lossy().into(),
                title: chat_id,
                last_active: mtime_secs(&p).map(|s| s.to_string()),
                alive: running,
                detail: "sqlite store.db; list-level".into(),
            }
        })
        .collect()
}

pub fn external_sessions() -> Vec<ExternalSession> {
    let mut out = codex_sessions();
    out.extend(cursor_sessions());
    out
}
