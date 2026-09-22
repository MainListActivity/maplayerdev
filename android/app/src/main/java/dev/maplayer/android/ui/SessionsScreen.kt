package dev.maplayer.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.maplayer.android.net.ManagedSession
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SessionsScreen(
    state: AppState,
    onOpen: (String) -> Unit,
    onProfiles: () -> Unit,
    onUnpair: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var showNew by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) { state.refreshSessions() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Sessions") },
                actions = {
                    IconButton(onClick = { scope.launch { state.refreshSessions() } }) {
                        Icon(Icons.Default.Refresh, contentDescription = "Refresh")
                    }
                    IconButton(onClick = onProfiles) {
                        Icon(Icons.Default.Person, contentDescription = "Profiles")
                    }
                    TextButton(onClick = onUnpair) { Text("Unpair") }
                },
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = { showNew = true }) {
                Icon(Icons.Default.Add, contentDescription = "New session")
            }
        },
    ) { pad ->
        state.error?.let {
            Text(it, Modifier.padding(16.dp), color = MaterialTheme.colorScheme.error)
        }
        LazyColumn(Modifier.padding(pad).fillMaxSize()) {
            item { SectionHeader("Managed (spawned via this server)") }
            items(state.sessions?.managed.orEmpty()) { s ->
                ManagedRow(
                    s,
                    onOpen = { onOpen(s.sessionId) },
                    onKill = {
                        scope.launch {
                            state.client.killSession(s.sessionId)
                            state.refreshSessions()
                        }
                    },
                )
            }
            item { SectionHeader("External (read-only)") }
            items(state.sessions?.external.orEmpty()) { s ->
                ListItem(
                    headlineContent = { Text(s.title ?: s.reference) },
                    supportingContent = {
                        Text("${s.provider} · ${s.detail} · ${if (s.alive) "running" else "idle"}")
                    },
                )
            }
        }
        if (showNew) {
            NewSessionSheet(state, onClose = { showNew = false }) { sessionId ->
                showNew = false
                scope.launch { state.refreshSessions() }
                onOpen(sessionId)
            }
        }
    }
}

@Composable
private fun SectionHeader(text: String) {
    Text(
        text,
        Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
    )
}

@Composable
private fun ManagedRow(s: ManagedSession, onOpen: () -> Unit, onKill: () -> Unit) {
    ListItem(
        modifier = Modifier.clickable(onClick = onOpen),
        headlineContent = { Text("${s.provider} · ${s.cwd}") },
        supportingContent = {
            Text("${s.state} · pid ${s.pid ?: "-"} · profile ${s.profile ?: "default"}")
        },
        trailingContent = {
            IconButton(onClick = onKill) {
                Icon(Icons.Default.Delete, contentDescription = "Kill")
            }
        },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NewSessionSheet(state: AppState, onClose: () -> Unit, onCreated: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    var provider by remember { mutableStateOf("codex") }
    var cwd by remember { mutableStateOf("") }
    var profile by remember { mutableStateOf("") }

    ModalBottomSheet(onDismissRequest = onClose) {
        Column(Modifier.padding(24.dp)) {
            Text("New session", style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(16.dp))
            Row {
                listOf("codex", "cursor").forEach { p ->
                    FilterChip(
                        selected = provider == p,
                        onClick = { provider = p },
                        label = { Text(p) },
                        modifier = Modifier.padding(end = 8.dp),
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = cwd,
                onValueChange = { cwd = it },
                label = { Text("Working directory (on host)") },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            if (provider == "codex") {
                val names = state.profiles?.profiles?.map { it.name }.orEmpty()
                var expanded by remember { mutableStateOf(false) }
                ExposedDropdownMenuBox(expanded = expanded, onExpandedChange = { expanded = it }) {
                    OutlinedTextField(
                        value = profile,
                        onValueChange = {},
                        readOnly = true,
                        label = { Text("Codex profile (blank = default)") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                        names.forEach { n ->
                            DropdownMenuItem(text = { Text(n) }, onClick = {
                                profile = n
                                expanded = false
                            })
                        }
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
            Button(
                enabled = cwd.isNotBlank() && !state.busy,
                onClick = {
                    scope.launch {
                        val id = state.client.newSession(provider, profile.ifBlank { null }, cwd)
                        onCreated(id)
                    }
                },
            ) { Text("Start") }
            Spacer(Modifier.height(24.dp))
        }
    }
}
