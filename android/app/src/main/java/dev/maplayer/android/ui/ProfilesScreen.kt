package dev.maplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/**
 * Codex account profiles (one CODEX_HOME each on the host). Creating a
 * profile returns a login instruction the user completes on the host side —
 * credentials never transit this device.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProfilesScreen(state: AppState, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var showNew by remember { mutableStateOf(false) }
    var loginNote by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) { state.refreshSessions() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Codex profiles") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = { showNew = true }) {
                Icon(Icons.Default.Add, contentDescription = "New profile")
            }
        },
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize().padding(16.dp)) {
            loginNote?.let {
                Text(it, color = MaterialTheme.colorScheme.tertiary)
                Spacer(Modifier.height(12.dp))
            }
            LazyColumn {
                items(state.profiles?.profiles.orEmpty()) { p ->
                    val isDefault = state.profiles?.default == p.name
                    ListItem(
                        headlineContent = { Text(p.name + if (isDefault) "  (default)" else "") },
                        supportingContent = { Text("${p.credential} · ${p.codexHome}") },
                        trailingContent = {
                            if (!isDefault) {
                                TextButton(onClick = {
                                    scope.launch {
                                        state.client.setDefaultProfile(p.name)
                                        state.refreshSessions()
                                    }
                                }) { Text("Set default") }
                            }
                        },
                    )
                }
            }
        }
        if (showNew) {
            NewProfileSheet(state, onClose = { showNew = false }) { result ->
                showNew = false
                loginNote = result.login.text
                scope.launch { state.refreshSessions() }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NewProfileSheet(
    state: AppState,
    onClose: () -> Unit,
    onCreated: (dev.maplayer.android.net.ProfileNewResult) -> Unit,
) {
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var credential by remember { mutableStateOf("chatgpt") }

    ModalBottomSheet(onDismissRequest = onClose) {
        Column(Modifier.padding(24.dp)) {
            Text("New profile", style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Name") },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Row {
                listOf("chatgpt" to "ChatGPT login", "api-key" to "API key").forEach { (v, label) ->
                    FilterChip(
                        selected = credential == v,
                        onClick = { credential = v },
                        label = { Text(label) },
                        modifier = Modifier.padding(end = 8.dp),
                    )
                }
            }
            Spacer(Modifier.height(16.dp))
            Button(
                enabled = name.isNotBlank() && !state.busy,
                onClick = {
                    scope.launch {
                        onCreated(state.client.newProfile(name.trim(), credential))
                    }
                },
            ) { Text("Create") }
            Spacer(Modifier.height(24.dp))
        }
    }
}
