package dev.maplayer.android.net

import android.content.Context
import computer.iroh.BiStream
import computer.iroh.Connection
import computer.iroh.Endpoint
import computer.iroh.EndpointBuilder
import dev.maplayer.android.store.ClientStore
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

private val ALPN = "maplayer/1".toByteArray()
private val READ_CHUNK = 64u * 1024u
private const val MAX_LINE = 1024 * 1024 // 1 MiB, mirrors the server cap

/**
 * Owns the client's iroh endpoint and speaks the maplayer wire protocol.
 * All methods are suspending; call from a coroutine.
 */
class IrohClient(context: Context) {
    private val store = ClientStore(context)
    private var endpoint: Endpoint? = null
    private var connection: Connection? = null
    private val connMutex = Mutex()
    private val idCounter = AtomicLong(0)

    val isPaired: Boolean get() = store.ticketJson != null
    val lastServerId: String? get() = store.lastServerId

    fun clearPairing() {
        store.clearPairing()
        connection = null
    }

    private suspend fun endpoint(): Endpoint =
        endpoint ?: EndpointBuilder().also { b ->
            b.applyN0()
            b.secretKey(store.secretKeyBytes())
            b.alpns(listOf(ALPN))
        }.bind().also { endpoint = it }

    /** Connect to the paired server (cached until a failure bumps it). */
    suspend fun connection(): Connection = connMutex.withLock {
        connection?.let { return@withLock it }
        val ticket = parseTicket(store.ticketJson ?: error("not paired"))
        val conn = endpoint().connect(ticket.toEndpointAddr(), ALPN)
        connection = conn
        conn
    }

    fun dropConnection() {
        connection = null
    }

    /**
     * Control-plane JSON-RPC: header {kind:"rpc"} then one request line;
     * the response is one line read until newline or end of stream.
     */
    suspend fun rpc(method: String, params: JsonObject = JsonObject(emptyMap())): JsonObject {
        val conn = connection()
        val stream = conn.openBi()
        try {
            val send = stream.send()
            send.writeAll(header("rpc"))
            val req = RpcRequest(id = idCounter.incrementAndGet(), method = method, params = params)
            send.writeAll(line(wireJson.encodeToString(RpcRequest.serializer(), req)))
            send.finish()
            val parsed = wireJson.decodeFromString(
                RpcResponse.serializer(),
                AcpStream(stream).readLine(),
            )
            parsed.error?.let { throw RpcException(it.code, it.message) }
            return parsed.result ?: JsonObject(emptyMap())
        } finally {
            stream.close()
        }
    }

    /**
     * Open an ACP passthrough stream to a managed session. The caller then
     * exchanges newline-delimited ACP JSON-RPC frames on the returned stream.
     */
    suspend fun openAcp(sessionId: String): AcpStream {
        val conn = connection()
        val stream = conn.openBi()
        stream.send().writeAll(header("acp", sessionId))
        return AcpStream(stream)
    }

    /** Trailing lines of an external session file (codex rollout jsonl). */
    suspend fun sessionTail(reference: String, lines: Int = 80): SessionTailResult =
        wireJson.decodeFromJsonElement(
            SessionTailResult.serializer(),
            rpc(
                "maplayer/session_tail",
                buildJsonObject {
                    put("reference", reference)
                    put("lines", lines)
                },
            ),
        )

    /**
     * First contact: connect with a scanned ticket + PIN, run pair_hello,
     * persist the ticket only on success.
     */
    suspend fun pair(ticketJson: String, label: String = "android") {
        val ticket = parseTicket(ticketJson)
        connMutex.withLock {
            connection = endpoint().connect(ticket.toEndpointAddr(), ALPN)
        }
        val result = rpc(
            "maplayer/pair_hello",
            buildJsonObject {
                put("pin", ticket.pin)
                put("label", label)
            },
        )
        store.ticketJson = ticketJson
        store.lastServerId = result["server_id"]?.jsonPrimitive?.content
    }

    // ---- convenience wrappers over control-plane methods ----

    suspend fun sessions(): SessionsResult =
        wireJson.decodeFromJsonElement(SessionsResult.serializer(), rpc("maplayer/sessions"))

    suspend fun newSession(provider: String, profile: String?, cwd: String): String =
        rpc(
            "maplayer/session_new",
            buildJsonObject {
                put("provider", provider)
                profile?.let { put("profile", it) }
                put("cwd", cwd)
            },
        )["session_id"]!!.jsonPrimitive.content

    suspend fun killSession(sessionId: String) {
        rpc("maplayer/session_kill", buildJsonObject { put("session_id", sessionId) })
    }

    suspend fun profiles(): ProfilesResult =
        wireJson.decodeFromJsonElement(ProfilesResult.serializer(), rpc("maplayer/profiles"))

    suspend fun newProfile(name: String, credential: String): ProfileNewResult =
        wireJson.decodeFromJsonElement(
            ProfileNewResult.serializer(),
            rpc(
                "maplayer/profile_new",
                buildJsonObject { put("name", name); put("credential", credential) },
            ),
        )

    suspend fun setDefaultProfile(name: String) {
        rpc("maplayer/profile_default", buildJsonObject { put("name", name) })
    }

    suspend fun ping(): String =
        rpc("maplayer/ping")["server_id"]?.jsonPrimitive?.content ?: ""

    // ---- framing helpers ----

    private fun header(kind: String, sessionId: String? = null): ByteArray =
        line(wireJson.encodeToString(StreamHeader.serializer(), StreamHeader(kind, 1, sessionId)))

    private fun line(text: String): ByteArray = (text + "\n").toByteArray()

    class RpcException(val code: Int, override val message: String) : Exception("rpc $code: $message")
}

/**
 * Buffered reader/writer over a BiStream. Keeps leftover bytes between
 * readLine calls — required on ACP streams where one QUIC read can carry
 * several newline-terminated frames — and enforces the 1 MiB line cap.
 */
class AcpStream internal constructor(private val stream: BiStream) {
    private var pending = ByteArray(0)

    /** Read one line without the trailing newline; empty string on EOF. */
    suspend fun readLine(): String {
        while (true) {
            val nl = pending.indexOf('\n'.code.toByte())
            if (nl >= 0) {
                val line = pending.copyOfRange(0, nl).decodeToString()
                pending = pending.copyOfRange(nl + 1, pending.size)
                return line
            }
            if (pending.size > MAX_LINE) throw IrohClient.RpcException(-32000, "line too long")
            val chunk = stream.recv().read(READ_CHUNK)
            if (chunk.isEmpty()) {
                val line = pending.decodeToString()
                pending = ByteArray(0)
                return line
            }
            pending += chunk
        }
    }

    suspend fun writeLine(text: String) {
        stream.send().writeAll((text + "\n").toByteArray())
    }

    fun close() = stream.close()
}
