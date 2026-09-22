package dev.maplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import computer.iroh.BiStream
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * Raw ACP stream view: shows newline-delimited ACP frames. A full client will
 * decode session/update notifications into chat bubbles; this skeleton keeps
 * the JSON lines visible for bring-up, plus a prompt box that sends the line
 * verbatim (usually an initialize/session-prompt JSON-RPC request).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TerminalScreen(state: AppState, sessionId: String, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    val lines = remember { mutableStateListOf<String>() }
    var input by remember { mutableStateOf("") }
    var stream by remember { mutableStateOf<BiStream?>(null) }
    var reader by remember { mutableStateOf<Job?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()

    LaunchedEffect(sessionId) {
        try {
            val s = state.client.openAcp(sessionId)
            stream = s
            reader = scope.launch {
                try {
                    while (true) {
                        val line = state.client.readLine(s)
                        if (line.isEmpty()) break
                        lines.add(line)
                    }
                } catch (e: Exception) {
                    err = e.message
                }
            }
        } catch (e: Exception) {
            err = e.message
        }
    }

    DisposableEffect(Unit) {
        onDispose {
            reader?.cancel()
            stream?.close()
        }
    }

    LaunchedEffect(lines.size) {
        if (lines.isNotEmpty()) listState.animateScrollToItem(lines.size - 1)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("ACP · ${sessionId.take(8)}") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize()) {
            err?.let { Text(it, Modifier.padding(8.dp), color = MaterialTheme.colorScheme.error) }
            LazyColumn(state = listState, modifier = Modifier.weight(1f).padding(horizontal = 12.dp)) {
                items(lines) { l ->
                    Text(l, fontFamily = FontFamily.Monospace, fontSize = 11.sp)
                }
            }
            Row(Modifier.fillMaxWidth().padding(12.dp)) {
                OutlinedTextField(
                    value = input,
                    onValueChange = { input = it },
                    modifier = Modifier.weight(1f),
                    placeholder = { Text("ACP frame (one JSON-RPC line)") },
                )
                IconButton(
                    enabled = stream != null && input.isNotBlank(),
                    onClick = {
                        val s = stream ?: return@IconButton
                        val payload = input + "\n"
                        input = ""
                        scope.launch {
                            try {
                                s.send().writeAll(payload.toByteArray())
                            } catch (e: Exception) {
                                err = e.message
                            }
                        }
                    },
                ) {
                    Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
                }
            }
        }
    }
}
