import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const $ = (id: string) => document.getElementById(id)!;

function renderTable(
  el: HTMLElement,
  head: string[],
  rows: string[][],
  onRow?: (row: string[]) => void,
) {
  el.textContent = "";
  const table = document.createElement("table");
  const headRow = document.createElement("tr");
  for (const h of head) {
    const th = document.createElement("th");
    th.textContent = h;
    headRow.appendChild(th);
  }
  table.appendChild(headRow);
  if (rows.length === 0) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = head.length;
    td.className = "muted";
    td.textContent = "none";
    tr.appendChild(td);
    table.appendChild(tr);
  }
  for (const r of rows) {
    const tr = document.createElement("tr");
    if (onRow) {
      tr.style.cursor = "pointer";
      tr.onclick = () => onRow(r);
    }
    for (const c of r) {
      const td = document.createElement("td");
      td.textContent = c;
      tr.appendChild(td);
    }
    table.appendChild(tr);
  }
  el.appendChild(table);
}

async function refreshStatus() {
  const st: any = await invoke("server_status");
  $("status").textContent = st.running
    ? `running · id ${st.endpoint_id ?? "?"}`
    : "stopped";
  $("info").textContent = st.running
    ? `endpoint_id: ${st.endpoint_id}\naddr: ${st.addr ?? "-"}\npairing: ${st.pairing ? "open" : "closed"}`
    : "";
  $("ticket").textContent = st.ticket ?? "";
}

async function refreshSessions() {
  try {
    const s: any = await invoke("sessions");
    const managed = (s.managed ?? []).map((m: any) =>
      [m.session_id, m.provider, m.profile ?? "-", m.state, m.pid ?? "-", m.cwd, m.created_at].map(String),
    );
    renderTable(
      $("managed"),
      ["session", "provider", "profile", "state", "pid", "cwd", "created"],
      managed,
      (row) => openSession(row[0]),
    );
    const external = (s.external ?? []).map((x: any) =>
      [x.provider, x.ref, x.title ?? "-", x.alive ? "alive" : "dead", x.last_active ?? "-", x.detail].map(String),
    );
    renderTable($("external"), ["provider", "ref", "title", "state", "last active", "detail"], external,
      (row) => openExternalTail(row[1]),
    );
    $("sessions-status").textContent = `${managed.length} managed · ${external.length} external`;
  } catch (e) {
    $("sessions-status").textContent = String(e);
  }
}

async function refreshProfiles() {
  try {
    const p: any = await invoke("profiles");
    const rows = (p.profiles ?? []).map((x: any) =>
      [x.name + (x.name === p.default ? " (default)" : ""), x.credential, x.codex_home].map(String),
    );
    renderTable($("profiles"), ["name", "credential", "codex_home"], rows);
    $("profiles-status").textContent = `${rows.length} profile${rows.length === 1 ? "" : "s"}`;
  } catch (e) {
    $("profiles-status").textContent = String(e);
  }
}

$("start-pair").onclick = async () => {
  $("status").textContent = "starting…";
  try {
    await invoke("start_server", { pair: true });
  } catch (e) {
    alert(String(e));
  }
  refreshStatus();
};

$("start").onclick = async () => {
  $("status").textContent = "starting…";
  try {
    await invoke("start_server", { pair: false });
  } catch (e) {
    alert(String(e));
  }
  refreshStatus();
};

$("stop").onclick = async () => {
  await invoke("stop_server");
  refreshStatus();
};

$("refresh").onclick = refreshSessions;
$("refresh-profiles").onclick = refreshProfiles;

// ---- session detail view ----

let openSessionId: string | null = null;
let acpReqId = 1;

function acpWrite(obj: any) {
  if (!openSessionId) return;
  invoke("session_send", { sessionId: openSessionId, line: JSON.stringify(obj) }).catch((e) =>
    logLine(`send error: ${e}`),
  );
}

function logLine(text: string) {
  const el = $("acp-log");
  el.textContent += text + "\n";
  el.scrollTop = el.scrollHeight;
}

async function openSession(id: string) {
  await closeSession();
  openSessionId = id;
  $("session-title").textContent = id.slice(0, 12);
  $("acp-log").textContent = "";
  $("perm-box").style.display = "none";
  $("session-view").style.display = "";
  try {
    await invoke("session_open", { sessionId: id });
  } catch (e) {
    logLine(`open error: ${e}`);
  }
}

async function openExternalTail(ref: string) {
  $("session-title").textContent = ref.split("/").pop() ?? ref;
  $("acp-log").textContent = "";
  $("perm-box").style.display = "none";
  $("session-view").style.display = "";
  try {
    const r: any = await invoke("session_tail", { reference: ref, lines: 80 });
    for (const l of r.lines ?? []) logLine(l);
    if ((r.lines ?? []).length === 0) logLine("(empty)");
  } catch (e) {
    logLine(`tail error: ${e}`);
  }
}

async function closeSession() {
  if (openSessionId) {
    await invoke("session_close", { sessionId: openSessionId }).catch(() => {});
    openSessionId = null;
  }
}

$("session-close").onclick = async () => {
  await closeSession();
  $("session-view").style.display = "none";
};

$("session-kill").onclick = async () => {
  if (!openSessionId) return;
  await invoke("session_kill", { sessionId: openSessionId }).catch((e) => logLine(String(e)));
  refreshSessions();
};

$("session-cancel").onclick = () =>
  acpWrite({ jsonrpc: "2.0", method: "session/cancel", params: { sessionId: openSessionId } });

$("prompt-send").onclick = () => {
  const input = $("prompt") as HTMLInputElement;
  const text = input.value.trim();
  if (!text) return;
  input.value = "";
  acpWrite({
    jsonrpc: "2.0",
    id: acpReqId++,
    method: "session/prompt",
    params: { sessionId: openSessionId, prompt: [{ type: "text", text }] },
  });
};

$("raw-send").onclick = () => {
  const input = $("raw-frame") as HTMLInputElement;
  const line = input.value.trim();
  if (!line) return;
  input.value = "";
  if (openSessionId)
    invoke("session_send", { sessionId: openSessionId, line }).catch((e) => logLine(String(e)));
};

await listen<{ session_id: string; line?: string; eof?: boolean }>("acp-line", (e) => {
  const p = e.payload;
  if (p.session_id !== openSessionId) return;
  if (p.eof) {
    logLine("— session stream ended —");
    return;
  }
  const line = p.line ?? "";
  logLine(line);
  try {
    const frame = JSON.parse(line);
    if (frame.method === "session/request_permission" && frame.id != null) {
      const box = $("perm-box");
      box.textContent = "";
      box.style.display = "";
      const title = document.createElement("div");
      title.className = "muted";
      title.textContent = `Permission: ${frame.params?.toolCall?.title ?? frame.params?.toolCall?.kind ?? "agent request"}`;
      box.appendChild(title);
      for (const opt of frame.params?.options ?? []) {
        const b = document.createElement("button");
        b.className = "secondary";
        b.style.marginRight = "6px";
        b.textContent = opt.name ?? opt.optionId;
        b.onclick = () => {
          box.style.display = "none";
          acpWrite({
            jsonrpc: "2.0",
            id: frame.id,
            result: { outcome: { outcome: "selected", optionId: opt.optionId } },
          });
        };
        box.appendChild(b);
      }
    }
  } catch {
    /* non-JSON line */
  }
});

await listen<string>("server-log", (e) => {
  const el = $("log");
  el.textContent += e.payload + "\n";
  el.scrollTop = el.scrollHeight;
});

await listen<string>("pairing-ticket", (e) => {
  $("ticket").textContent = e.payload;
});

await listen("server-event", () => refreshStatus());
refreshStatus();
refreshSessions();
refreshProfiles();
