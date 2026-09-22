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
import dev.maplayer.android.net.AcpStream
import dev.maplayer.android.net.wireJson
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.atomic.AtomicLong

/**
 * ACP session view: live session/update lines plus structured controls —
 * a prompt box (session/prompt), a Cancel button (session/cancel), and
 * option buttons when the agent sends session/request_permission. A
 * collapsible raw-frame input remains for bring-up debugging.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TerminalScreen(state: AppState, sessionId: String, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    val lines = remember { mutableStateListOf<String>() }
    var input by remember { mutableStateOf("") }
    var stream by remember { mutableStateOf<AcpStream?>(null) }
    var reader by remember { mutableStateOf<Job?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    var permissionReq by remember { mutableStateOf<JsonObject?>(null) }
    val reqId = remember { AtomicLong(1000) }
    val listState = rememberLazyListState()

    fun sendRaw(text: String) {
        val s = stream ?: return
        scope.launch {
            try {
                s.writeLine(text)
            } catch (e: Exception) {
                err = e.message
            }
        }
    }

    fun sendPrompt(text: String) = sendRaw(
        """{"jsonrpc":"2.0","id":${reqId.incrementAndGet()},"method":"session/prompt","params":{"sessionId":"$sessionId","prompt":[{"type":"text","text":${wireEncode(text)}}]}}""",
    )

    LaunchedEffect(sessionId) {
        try {
            val s = state.client.openAcp(sessionId)
            stream = s
            reader = scope.launch {
                try {
                    while (true) {
                        val line = s.readLine()
                        if (line.isEmpty()) break
                        lines.add(line)
                        runCatching { wireJson.parseToJsonElement(line).jsonObject }.getOrNull()
                            ?.let { f ->
                                if (f["method"]?.jsonPrimitive?.content ==
                                    "session/request_permission" && f["id"] != null
                                ) {
                                    permissionReq = f
                                }
                            }
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
                actions = {
                    TextButton(onClick = {
                        sendRaw("""{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"$sessionId"}}""")
                    }) { Text("Cancel") }
                },
            )
        },
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize()) {
            err?.let { Text(it, Modifier.padding(8.dp), color = MaterialTheme.colorScheme.error) }
            permissionReq?.let { req ->
                Card(Modifier.fillMaxWidth().padding(8.dp)) {
                    Column(Modifier.padding(12.dp)) {
                        Text(
                            "Permission: " +
                                (req["params"]?.jsonObject?.get("toolCall")?.jsonObject
                                    ?.get("title")?.jsonPrimitive?.content ?: "agent request"),
                            style = MaterialTheme.typography.labelLarge,
                        )
                        Spacer(Modifier.height(8.dp))
                        Row {
                            val options = req["params"]?.jsonObject?.get("options")?.jsonArray
                                .orEmpty()
                            options.forEach { opt ->
                                val id = opt.jsonObject["optionId"]?.jsonPrimitive?.content ?: ""
                                val name = opt.jsonObject["name"]?.jsonPrimitive?.content ?: id
                                TextButton(onClick = {
                                    permissionReq = null
                                    sendRaw(
                                        """{"jsonrpc":"2.0","id":${req["id"]},"result":{"outcome":{"outcome":"selected","optionId":"$id"}}}""",
                                    )
                                }) { Text(name) }
                            }
                        }
                    }
                }
            }
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
                    placeholder = { Text("Prompt (or a raw ACP frame)") },
                )
                IconButton(
                    enabled = stream != null && input.isNotBlank(),
                    onClick = {
                        val text = input
                        input = ""
                        if (text.trimStart().startsWith("{")) sendRaw(text) else sendPrompt(text)
                    },
                ) {
                    Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
                }
            }
        }
    }
}

private fun wireEncode(text: String): String =
    buildString {
        append('"')
        for (c in text) {
            when (c) {
                '"' -> append("\\\"")
                '\\' -> append("\\\\")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> append(c)
            }
        }
        append('"')
    }
