import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const $ = (id: string) => document.getElementById(id)!;

function renderTable(el: HTMLElement, head: string[], rows: string[][]) {
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
    renderTable($("managed"), ["session", "provider", "profile", "state", "pid", "cwd", "created"], managed);
    const external = (s.external ?? []).map((x: any) =>
      [x.provider, x.ref, x.title ?? "-", x.alive ? "alive" : "dead", x.last_active ?? "-", x.detail].map(String),
    );
    renderTable($("external"), ["provider", "ref", "title", "state", "last active", "detail"], external);
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
