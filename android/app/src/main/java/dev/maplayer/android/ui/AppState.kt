package dev.maplayer.android.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import dev.maplayer.android.net.IrohClient
import dev.maplayer.android.net.ProfilesResult
import dev.maplayer.android.net.SessionsResult

/** Shared screen state + error surface. */
class AppState(val client: IrohClient) {
    var sessions by mutableStateOf<SessionsResult?>(null)
    var profiles by mutableStateOf<ProfilesResult?>(null)
    var busy by mutableStateOf(false)
    var error by mutableStateOf<String?>(null)

    suspend inline fun run(crossinline block: suspend () -> Unit) {
        busy = true
        error = null
        try {
            block()
        } catch (e: Exception) {
            error = e.message ?: e.toString()
        } finally {
            busy = false
        }
    }

    suspend fun refreshSessions() = run {
        sessions = client.sessions()
        profiles = client.profiles()
    }
}
