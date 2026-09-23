package dev.maplayer.android.store

import android.content.Context
import android.util.Base64

/**
 * Persists the client's iroh secret key and the paired server ticket.
 * The secret key IS the client identity — it never leaves this device.
 */
class ClientStore(context: Context) {
    private val prefs = context.getSharedPreferences("maplayer", Context.MODE_PRIVATE)

    fun secretKeyBytes(): ByteArray {
        val existing = prefs.getString(KEY_SECRET, null)
        if (existing != null) return Base64.decode(existing, Base64.DEFAULT)
        val key = computer.iroh.SecretKey.generate()
        prefs.edit()
            .putString(KEY_SECRET, Base64.encodeToString(key.toBytes(), Base64.DEFAULT))
            .apply()
        return key.toBytes()
    }

    var ticketJson: String?
        get() = prefs.getString(KEY_TICKET, null)
        set(value) = prefs.edit().putString(KEY_TICKET, value).apply()

    var lastServerId: String?
        get() = prefs.getString(KEY_SERVER_ID, null)
        set(value) = prefs.edit().putString(KEY_SERVER_ID, value).apply()

    fun clearPairing() {
        prefs.edit().remove(KEY_TICKET).remove(KEY_SERVER_ID).apply()
    }

    private companion object {
        const val KEY_SECRET = "secret_key"
        const val KEY_TICKET = "ticket_json"
        const val KEY_SERVER_ID = "server_id"
    }
}
