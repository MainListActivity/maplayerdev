package dev.maplayer.android.net

import computer.iroh.EndpointAddr
import computer.iroh.EndpointId
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive

val wireJson = Json { ignoreUnknownKeys = true }

// ---- Stream headers (see server/proto/PROTOCOL.md) ----

@Serializable
data class StreamHeader(
    val kind: String,
    val v: Int = 1,
    @SerialName("session_id") val sessionId: String? = null,
)

// ---- Pair ticket emitted by `maplayer-server pair` ----

@Serializable
data class PairTicket(
    val addr: TicketAddr,
    val pin: String,
) {
    @Serializable
    data class TicketAddr(
        val id: String,
        val addrs: List<kotlinx.serialization.json.JsonObject> = emptyList(),
    )

    fun toEndpointAddr(): EndpointAddr {
        val relay = addrsOf("Relay").firstOrNull()
        val ips = addrsOf("Ip")
        return EndpointAddr(EndpointId.fromString(addr.id), relay, ips)
    }

    private fun addrsOf(kind: String): List<String> =
        addr.addrs.mapNotNull { it[kind]?.jsonPrimitive?.content }
}

fun parseTicket(jsonText: String): PairTicket =
    wireJson.decodeFromString(jsonText)

// ---- JSON-RPC 2.0 ----

@Serializable
data class RpcRequest(
    val jsonrpc: String = "2.0",
    val id: Long,
    val method: String,
    val params: JsonObject = JsonObject(emptyMap()),
)

@Serializable
data class RpcResponse(
    val jsonrpc: String? = null,
    val id: Long? = null,
    val result: JsonObject? = null,
    val error: RpcError? = null,
) {
    @Serializable
    data class RpcError(val code: Int, val message: String)
}

// ---- Control-plane results ----

@Serializable
data class ManagedSession(
    @SerialName("session_id") val sessionId: String,
    val provider: String,
    val profile: String? = null,
    val cwd: String,
    val state: String,
    val pid: Long? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class ExternalSession(
    val provider: String,
    @SerialName("ref") val reference: String,
    val title: String? = null,
    @SerialName("last_active") val lastActive: String? = null,
    val alive: Boolean,
    val detail: String,
)

@Serializable
data class SessionsResult(
    val managed: List<ManagedSession> = emptyList(),
    val external: List<ExternalSession> = emptyList(),
)

@Serializable
data class Profile(
    val name: String,
    val credential: String,
    @SerialName("codex_home") val codexHome: String,
)

@Serializable
data class ProfilesResult(
    val profiles: List<Profile> = emptyList(),
    val default: String? = null,
)

@Serializable
data class LoginInstruction(val kind: String, val text: String)

@Serializable
data class ProfileNewResult(
    val name: String,
    @SerialName("codex_home") val codexHome: String,
    val login: LoginInstruction,
)
