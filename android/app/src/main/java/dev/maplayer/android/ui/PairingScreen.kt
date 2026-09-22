package dev.maplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/**
 * Paste the pairing ticket JSON printed by `maplayer-server pair`
 * (`{"addr":{...},"pin":"123456"}`). QR scanning is a follow-up; the desktop
 * app can also display this ticket as a QR code.
 */
@Composable
fun PairingScreen(state: AppState, onPaired: () -> Unit) {
    var ticket by remember { mutableStateOf("") }
    var err by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    Scaffold { pad ->
        Column(
            Modifier.padding(pad).fillMaxSize().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text("Pair with host", style = MaterialTheme.typography.headlineMedium)
            Spacer(Modifier.height(8.dp))
            Text(
                "Run `maplayer-server pair` on your dev machine and paste the ticket JSON here.",
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(24.dp))
            OutlinedTextField(
                value = ticket,
                onValueChange = { ticket = it },
                modifier = Modifier.fillMaxWidth(),
                minLines = 5,
                label = { Text("Pairing ticket JSON") },
            )
            Spacer(Modifier.height(16.dp))
            err?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            Button(
                enabled = ticket.isNotBlank() && !state.busy,
                onClick = {
                    scope.launch {
                        err = null
                        try {
                            state.client.pair(ticket.trim())
                            onPaired()
                        } catch (e: Exception) {
                            err = e.message ?: e.toString()
                        }
                    }
                },
            ) { Text(if (state.busy) "Pairing…" else "Pair") }
        }
    }
}
