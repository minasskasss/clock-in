package io.github.minasskasss.clockin.alarm

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONArray
import org.json.JSONObject
import java.time.OffsetDateTime

/**
 * One alarm for one person, as computed by Rust (`clockin-core::alarm_plan`).
 * `firesAt` is kept exactly as received, because `check_alarm` compares it
 * with the server's plan (DECISIONS 2026-10-04).
 */
internal data class PlanItem(
    val itemId: String,
    val firesAt: String,
    val firesAtMs: Long,
    /** "in" or "out". */
    val kind: String,
    val name: String,
) {
    /** "Already rung / stopped" state is keyed by (item_id, fires_at). */
    val key: String get() = "$itemId|$firesAtMs"

    fun toJson(): JSONObject = JSONObject()
        .put("itemId", itemId)
        .put("firesAt", firesAt)
        .put("kind", kind)
        .put("name", name)

    companion object {
        fun parseInstantMs(iso: String): Long? =
            try {
                OffsetDateTime.parse(iso).toInstant().toEpochMilli()
            } catch (e: Exception) {
                null
            }

        fun of(itemId: String, firesAt: String, kind: String, name: String): PlanItem? {
            val ms = parseInstantMs(firesAt) ?: return null
            if (kind != "in" && kind != "out") return null
            return PlanItem(itemId, firesAt, ms, kind, name)
        }

        /** Rust's own form (`toJson`) or the server's `get_plan` item. */
        fun fromJson(o: JSONObject): PlanItem? = of(
            o.optString("itemId", o.optString("item_id")),
            o.optString("firesAt", o.optString("fires_at")),
            o.optString("kind"),
            o.optString("name", o.optString("display_name")),
        )
    }
}

internal data class Plan(
    /** The server `config_version` the plan was computed from. */
    val configVersion: Long,
    /** How far the plan reaches, or null if unknown. */
    val horizonEndMs: Long?,
    val items: List<PlanItem>,
)

/** The alarm cycle this device is in (ringing or silent between rings). */
internal data class Active(
    val items: List<PlanItem>,
    /** When the current ring started. */
    val ringStartedAtMs: Long,
    /** When a silent cycle rings again; null while ringing. */
    val reringAtMs: Long?,
)

/**
 * Everything the alarm code keeps, in device-protected storage so the
 * receivers also work after a reboot before the phone is first unlocked:
 * alarm times, kinds and staff names, the alert mode, the public server
 * config and small bookkeeping. Never the secrets ([Secrets]).
 */
internal class Store private constructor(private val prefs: SharedPreferences) {
    companion object {
        private const val FILE = "clockin_alarm"
        /** Read-modify-write sections take this lock. */
        val LOCK = Any()

        fun get(context: Context): Store {
            val storage = context.applicationContext.createDeviceProtectedStorageContext()
            return Store(storage.getSharedPreferences(FILE, Context.MODE_PRIVATE))
        }

        const val MODE_RING = "ring"
        const val MODE_NOTIFICATION = "notification"

        /** Handled alarms are forgotten after a day; they can't fire again anyway. */
        private const val HANDLED_KEEP_MS = 24L * 60 * 60 * 1000
    }

    // --- Plan -------------------------------------------------------------

    fun plan(): Plan {
        val raw = prefs.getString("plan", null) ?: return Plan(0, null, emptyList())
        return try {
            val o = JSONObject(raw)
            val items = o.optJSONArray("items") ?: JSONArray()
            Plan(
                o.optLong("configVersion", 0),
                if (o.has("horizonEndMs")) o.getLong("horizonEndMs") else null,
                (0 until items.length()).mapNotNull { PlanItem.fromJson(items.getJSONObject(it)) },
            )
        } catch (e: Exception) {
            Plan(0, null, emptyList())
        }
    }

    fun setPlan(plan: Plan) {
        val items = JSONArray()
        plan.items.forEach { items.put(it.toJson()) }
        val o = JSONObject().put("configVersion", plan.configVersion).put("items", items)
        plan.horizonEndMs?.let { o.put("horizonEndMs", it) }
        prefs.edit().putString("plan", o.toString()).commit()
    }

    fun clearPlan() {
        prefs.edit().remove("plan").remove("active").commit()
    }

    // --- Per-device settings ------------------------------------------------

    fun alertMode(): String =
        if (prefs.getString("alert_mode", MODE_RING) == MODE_NOTIFICATION) MODE_NOTIFICATION else MODE_RING

    fun setAlertMode(mode: String) {
        prefs.edit().putString("alert_mode", if (mode == MODE_NOTIFICATION) MODE_NOTIFICATION else MODE_RING).commit()
    }

    fun serverApi(): ServerApi? {
        val url = prefs.getString("server_url", null) ?: return null
        val key = prefs.getString("server_key", null) ?: return null
        return ServerApi(url, key)
    }

    fun setServerConfig(url: String, publishableKey: String) {
        prefs.edit().putString("server_url", url).putString("server_key", publishableKey).commit()
    }

    fun oemDone(): Boolean = prefs.getBoolean("oem_done", false)

    fun setOemDone(done: Boolean) {
        prefs.edit().putBoolean("oem_done", done).commit()
    }

    fun notificationRequests(): Int = prefs.getInt("notification_requests", 0)

    fun countNotificationRequest() {
        prefs.edit().putInt("notification_requests", notificationRequests() + 1).commit()
    }

    // --- The alarm screen's theme (SPEC §3) ----------------------------------

    /** `auto`, `system`, `light` or `dark`, as chosen in the app. */
    fun theme(): String = prefs.getString("theme", "auto") ?: "auto"

    /** When the automatic theme is dark, `[start, end)` in epoch ms (computed by Rust). */
    fun darkWindows(): List<Pair<Long, Long>> {
        val raw = prefs.getString("dark_windows", null) ?: return emptyList()
        return try {
            val a = JSONArray(raw)
            (0 until a.length()).map { a.getJSONArray(it).let { w -> w.getLong(0) to w.getLong(1) } }
        } catch (e: Exception) {
            emptyList()
        }
    }

    fun setTheme(theme: String, darkWindows: List<Pair<Long, Long>>) {
        val a = JSONArray()
        darkWindows.forEach { (start, end) -> a.put(JSONArray().put(start).put(end)) }
        prefs.edit().putString("theme", theme).putString("dark_windows", a.toString()).commit()
    }

    // --- Diagnostics («Διαγνωστικά») ----------------------------------------

    fun recordRefresh(atMs: Long, ok: Boolean) {
        prefs.edit().putLong("last_refresh_at", atMs).putBoolean("last_refresh_ok", ok).commit()
    }

    /** When the background refresh last ran, and whether it reached the server. */
    fun lastRefresh(): Pair<Long, Boolean>? =
        if (prefs.contains("last_refresh_at")) {
            prefs.getLong("last_refresh_at", 0) to prefs.getBoolean("last_refresh_ok", false)
        } else {
            null
        }

    /**
     * An alarm rang at `atMs`: `notification` (posted, screen not seen yet),
     * `fullScreen`, `opened` (from the notification) or `notificationMode`.
     */
    fun recordAlarm(atMs: Long, how: String) {
        prefs.edit().putLong("last_alarm_at", atMs).putString("last_alarm_how", how).commit()
    }

    /** The alarm screen opened: by itself (full screen) or from the notification. */
    fun alarmScreenShown(fullScreen: Boolean) {
        val how = prefs.getString("last_alarm_how", null) ?: return
        if (how == "notification" || (fullScreen && how == "opened")) {
            prefs.edit().putString("last_alarm_how", if (fullScreen) "fullScreen" else "opened").commit()
        }
    }

    fun lastAlarm(): Pair<Long, String>? {
        val how = prefs.getString("last_alarm_how", null) ?: return null
        return prefs.getLong("last_alarm_at", 0) to how
    }

    /** The app crashed (written synchronously: the process is ending). */
    fun recordCrash(atMs: Long, version: String, error: String) {
        prefs.edit()
            .putLong("last_crash_at", atMs)
            .putString("last_crash_version", version)
            .putString("last_crash_error", error)
            .commit()
    }

    /** The last crash: time, app version and error. */
    fun lastCrash(): Triple<Long, String, String>? {
        val error = prefs.getString("last_crash_error", null) ?: return null
        return Triple(prefs.getLong("last_crash_at", 0), prefs.getString("last_crash_version", "") ?: "", error)
    }

    // --- Handled alarms -----------------------------------------------------

    fun handledKeys(): Set<String> = handled().keys

    private fun handled(): MutableMap<String, Long> {
        val raw = prefs.getString("handled", null) ?: return mutableMapOf()
        return try {
            val o = JSONObject(raw)
            o.keys().asSequence().associateWith { o.getLong(it) }.toMutableMap()
        } catch (e: Exception) {
            mutableMapOf()
        }
    }

    /** Remembers these alarms as rung and stopped or ended on this device. */
    fun markHandled(items: Collection<PlanItem>, nowMs: Long) {
        val map = handled()
        map.entries.removeAll { nowMs - it.value > HANDLED_KEEP_MS }
        items.forEach { map[it.key] = nowMs }
        val o = JSONObject()
        map.forEach { (k, v) -> o.put(k, v) }
        prefs.edit().putString("handled", o.toString()).commit()
    }

    // --- The alarm cycle in progress ---------------------------------------

    fun active(): Active? {
        val raw = prefs.getString("active", null) ?: return null
        return try {
            val o = JSONObject(raw)
            val items = o.getJSONArray("items")
            Active(
                (0 until items.length()).mapNotNull { PlanItem.fromJson(items.getJSONObject(it)) },
                o.getLong("ringStartedAtMs"),
                if (o.has("reringAtMs")) o.getLong("reringAtMs") else null,
            ).takeIf { it.items.isNotEmpty() }
        } catch (e: Exception) {
            null
        }
    }

    fun setActive(active: Active?) {
        if (active == null || active.items.isEmpty()) {
            prefs.edit().remove("active").commit()
            return
        }
        val items = JSONArray()
        active.items.forEach { items.put(it.toJson()) }
        val o = JSONObject().put("items", items).put("ringStartedAtMs", active.ringStartedAtMs)
        active.reringAtMs?.let { o.put("reringAtMs", it) }
        prefs.edit().putString("active", o.toString()).commit()
    }

    // --- Scheduled exact alarms (to cancel them on the next reschedule) -----

    fun scheduledCodes(): List<Int> {
        val raw = prefs.getString("scheduled", null) ?: return emptyList()
        return try {
            val a = JSONArray(raw)
            (0 until a.length()).map { a.getInt(it) }
        } catch (e: Exception) {
            emptyList()
        }
    }

    fun setScheduledCodes(codes: List<Int>) {
        val a = JSONArray()
        codes.forEach { a.put(it) }
        prefs.edit().putString("scheduled", a.toString()).commit()
    }
}
