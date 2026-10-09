package io.github.minasskasss.clockin.alarm

import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * A minimal client for the three RPCs the background code needs
 * (ARCHITECTURE §6, §10): `get_version`, `get_plan` and `check_alarm`. The
 * publishable key goes in the `apikey` header only (DECISIONS 2026-10-05).
 */
internal class ServerApi(baseUrl: String, private val publishableKey: String) {
    private val rpcBase = baseUrl.trimEnd('/') + "/rest/v1/rpc/"

    sealed class Answer {
        class Ok(val body: JSONObject) : Answer()
        /** The server answered `{ ok: false, error }`. */
        class Rejected(val error: String) : Answer() {
            val unpaired: Boolean get() = error == "bad_secret" || error == "revoked"
        }
        /** No usable answer in time: offline, DNS, TLS, HTTP error. */
        object Unreachable : Answer()
    }

    /** One RPC, giving up after [timeoutMs] in total (including DNS). */
    fun call(function: String, args: JSONObject, timeoutMs: Long): Answer {
        val executor = Executors.newSingleThreadExecutor()
        return try {
            executor.submit(Callable { post(function, args, timeoutMs.toInt()) }).get(timeoutMs, TimeUnit.MILLISECONDS)
        } catch (e: Exception) {
            Answer.Unreachable
        } finally {
            executor.shutdownNow()
        }
    }

    private fun post(function: String, args: JSONObject, timeoutMs: Int): Answer {
        val connection = URL(rpcBase + function).openConnection() as HttpURLConnection
        try {
            connection.requestMethod = "POST"
            connection.connectTimeout = timeoutMs
            connection.readTimeout = timeoutMs
            connection.doOutput = true
            connection.setRequestProperty("apikey", publishableKey)
            connection.setRequestProperty("Content-Type", "application/json")
            connection.setRequestProperty("Accept", "application/json")
            connection.outputStream.use { it.write(args.toString().toByteArray(Charsets.UTF_8)) }
            if (connection.responseCode !in 200..299) return Answer.Unreachable
            val body = JSONObject(connection.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() })
            return if (body.optBoolean("ok", false)) Answer.Ok(body) else Answer.Rejected(body.optString("error"))
        } finally {
            connection.disconnect()
        }
    }

    fun getVersion(secret: String): Answer =
        call("get_version", JSONObject().put("p_secret", secret), 15_000)

    fun getPlan(secret: String): Answer =
        call("get_plan", JSONObject().put("p_secret", secret), 20_000)

    /**
     * Sends `(item_id, fires_at)` pairs; the server answers `due` per item
     * (same id at the same time still in its plan, and no live mark).
     */
    fun checkAlarm(secret: String, items: List<PlanItem>, timeoutMs: Long): Answer {
        val list = JSONArray()
        items.forEach { list.put(JSONObject().put("item_id", it.itemId).put("fires_at", it.firesAt)) }
        return call("check_alarm", JSONObject().put("p_secret", secret).put("p_items", list), timeoutMs)
    }
}
