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

/// Walk `rel` under ~ up to `depth`, keep files matching `pred`, newest
/// first, capped at `limit`, and map each path to an ExternalSession via
/// `describe` (which supplies provider-specific title and detail).
fn discover(
    rel: &str,
    depth: usize,
    pred: impl Fn(&PathBuf) -> bool,
    limit: usize,
    pgrep_pattern: &str,
    describe: impl Fn(&PathBuf) -> (String, Option<String>, String),
) -> Vec<ExternalSession> {
    let mut files = Vec::new();
    if let Some(root) = home().map(|h| h.join(rel)) {
        walk(&root, depth, &mut files);
    }
    files.retain(|p| pred(p));
    files.sort_by_key(|p| std::cmp::Reverse(mtime_secs(p).unwrap_or(0)));
    files.truncate(limit);
    let running = pgrep(pgrep_pattern);
    files
        .into_iter()
        .map(|p| {
            let (provider, title, detail) = describe(&p);
            ExternalSession {
                provider,
                reference: p.to_string_lossy().into(),
                title,
                last_active: mtime_secs(&p).map(|s| s.to_string()),
                alive: running,
                detail,
            }
        })
        .collect()
}

pub fn codex_sessions() -> Vec<ExternalSession> {
    discover(
        ".codex/sessions",
        4,
        |p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("rollout-"))
                .unwrap_or(false)
                && p.extension().map(|e| e == "jsonl").unwrap_or(false)
        },
        CODEX_LIMIT,
        "codex",
        |p| {
            (
                "codex".into(),
                p.file_name().map(|n| n.to_string_lossy().into()),
                "rollout jsonl; tail-capable".into(),
            )
        },
    )
}

pub fn cursor_sessions() -> Vec<ExternalSession> {
    discover(
        ".cursor/chats",
        3,
        |p| p.file_name().map(|n| n == "store.db").unwrap_or(false),
        CURSOR_LIMIT,
        "cursor-agent|agent acp",
        |p| {
            (
                "cursor".into(),
                p.parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().into()),
                "sqlite store.db; list-level".into(),
            )
        },
    )
}

pub fn external_sessions() -> Vec<ExternalSession> {
    let mut out = codex_sessions();
    out.extend(cursor_sessions());
    out
}

/// Read the last `n` lines of an external Codex rollout file. `reference`
/// must resolve to a `rollout-*.jsonl` under `~/.codex/sessions` — anything
/// else (cursor store.db, arbitrary paths) is refused.
pub fn tail_rollout(reference: &str, n: usize) -> anyhow::Result<(Vec<String>, u64)> {
    use anyhow::{bail, Context};
    let path = std::fs::canonicalize(reference).context("session file not found")?;
    let root = home()
        .map(|h| h.join(".codex/sessions"))
        .and_then(|r| std::fs::canonicalize(&r).ok())
        .context("no ~/.codex/sessions")?;
    let name_ok = path
        .file_name()
        .map(|f| {
            let f = f.to_string_lossy();
            f.starts_with("rollout-") && f.ends_with(".jsonl")
        })
        .unwrap_or(false);
    if !path.starts_with(&root) || !name_ok {
        bail!("reference is not a codex rollout file");
    }
    let bytes = std::fs::read(&path)?;
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(n.max(1));
    let offset: u64 = lines[..start].iter().map(|l| l.len() as u64 + 1).sum();
    let out = lines[start..]
        .iter()
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect();
    Ok((out, offset))
}
