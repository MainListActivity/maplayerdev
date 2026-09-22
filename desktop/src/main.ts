import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const $ = (id: string) => document.getElementById(id)!;

async function refreshStatus() {
  const st: any = await invoke("server_status");
  $("status").textContent = st.running
    ? `running · id ${st.endpoint_id ?? "?"}`
    : "stopped";
  $("info").textContent = st.running
    ? `endpoint_id: ${st.endpoint_id}\naddr: ${st.addr ?? "-"}\npairing: ${st.pairing ? "open" : "closed"}`
    : "";
  if (st.ticket) $("ticket").textContent = st.ticket;
}

async function refreshSessions() {
  try {
    const s: any = await invoke("sessions");
    $("sessions").textContent = JSON.stringify(s, null, 2);
  } catch (e) {
    $("sessions").textContent = String(e);
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
  $("ticket").textContent = "";
  refreshStatus();
};

$("refresh").onclick = refreshSessions;

await listen<string>("server-log", (e) => {
  const el = $("log");
  el.textContent += e.payload + "\n";
  el.scrollTop = el.scrollHeight;
});

await listen("server-event", () => refreshStatus());
refreshStatus();
refreshSessions();
