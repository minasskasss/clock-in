package io.github.minasskasss.clockin.alarm

import android.app.Activity
import android.app.AlertDialog
import android.webkit.WebView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class ItemArg {
    lateinit var itemId: String
    lateinit var firesAt: String
    lateinit var kind: String
    lateinit var name: String
}

@InvokeArg
class WindowArg {
    var start: Long = 0
    var end: Long = 0
}

@InvokeArg
class SetPlanArgs {
    lateinit var items: Array<ItemArg>
    lateinit var alertMode: String
    var configVersion: Long = 0
    var horizonEnd: String? = null
    var theme: String = "auto"
    var darkWindows: Array<WindowArg> = emptyArray()
}

@InvokeArg
class KindArgs {
    lateinit var kind: String
}

@InvokeArg
class NameArgs {
    lateinit var name: String
}

@InvokeArg
class SecretArgs {
    lateinit var name: String
    lateinit var value: String
}

@InvokeArg
class ServerConfigArgs {
    lateinit var url: String
    lateinit var publishableKey: String
}

@InvokeArg
class DoneArgs {
    var done: Boolean = false
}

/**
 * The Rust side's bridge (ARCHITECTURE §10). Called only from Rust
 * (`plugins/clockin-alarm/src/mobile.rs`), never from the webview. The
 * background code (receivers, service, worker) runs without this class.
 */
@TauriPlugin
class AlarmPlugin(private val activity: Activity) : Plugin(activity) {
    private val context get() = activity.applicationContext

    /**
     * An Android System WebView older than the app's screens need (often on
     * phones that don't update it): a native warning each time the app
     * opens, because those screens themselves may not show.
     */
    override fun load(webView: WebView) {
        super.load(webView)
        if (!Permissions.webViewTooOld()) return
        activity.runOnUiThread {
            val version = Permissions.webView()?.versionName ?: ""
            AlertDialog.Builder(activity)
                .setTitle(R.string.clockin_android_webViewOldTitle)
                .setMessage(activity.getString(R.string.clockin_android_webViewOldBody, version))
                .setPositiveButton(R.string.clockin_android_webViewOldUpdate) { _, _ -> Permissions.openWebViewStore(activity) }
                .setNegativeButton(R.string.clockin_android_webViewOldLater, null)
                .show()
        }
    }

    /** The plan Rust computed, and this device's alert mode. */
    @Command
    fun setPlan(invoke: Invoke) {
        val args = invoke.parseArgs(SetPlanArgs::class.java)
        val items = args.items.mapNotNull { PlanItem.of(it.itemId, it.firesAt, it.kind, it.name) }
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            store.setAlertMode(args.alertMode)
            store.setPlan(Plan(args.configVersion, args.horizonEnd?.let { PlanItem.parseInstantMs(it) }, items))
            store.setTheme(args.theme, args.darkWindows.map { it.start to it.end })
        }
        Notifications.ensureChannels(context)
        Ring.onPlanChanged(context)
        AlarmScheduler.reschedule(context)
        PlanWorker.ensurePeriodic(context)
        invoke.resolve()
    }

    @Command
    fun permissionStatus(invoke: Invoke) {
        val result = JSObject()
        val status = Permissions.status(context)
        status.forEach { (k, v) -> result.put(k, v) }
        if (listOf("notifications", "exactAlarms", "fullScreen", "battery", "unusedApps").all { status[it] == true }) {
            Notifications.cancel(context, Notifications.ID_SETUP)
        }
        Ring.ensureShown(context)
        invoke.resolve(result)
    }

    /** The ring-mode alarm in progress, for the «Σταμάτημα» bar on Today. */
    @Command
    fun alarmStatus(invoke: Invoke) {
        val active = Store.get(context).active()
        val result = JSObject()
        result.put("active", active != null)
        result.put("ringing", RingService.running?.isRinging == true)
        val names = { kind: String -> JSArray().apply { active?.items?.filter { it.kind == kind }?.map { it.name }?.distinct()?.forEach { put(it) } } }
        result.put("checkIn", names("in"))
        result.put("checkOut", names("out"))
        invoke.resolve(result)
    }

    /** «Σταμάτημα» on Today: the same as on the alarm screen (marks nobody). */
    @Command
    fun stopAlarm(invoke: Invoke) {
        Ring.stop(context)
        invoke.resolve()
    }

    /** The «Διαγνωστικά» view: the phone, the background refresh and the alarms. No secrets. */
    @Command
    fun diagnostics(invoke: Invoke) {
        val store = Store.get(context)
        val result = JSObject()
        Permissions.phone().forEach { (k, v) -> result.put(k, v) }
        store.lastRefresh()?.let { (at, ok) ->
            result.put("lastRefreshAt", at)
            result.put("lastRefreshOk", ok)
        }
        AlarmScheduler.nextAt(context)?.let { result.put("nextAlarmAt", it) }
        store.lastAlarm()?.let { (at, how) ->
            result.put("lastAlarmAt", at)
            result.put("lastAlarmHow", how)
        }
        store.lastCrash()?.let { (at, version, error) ->
            result.put("lastCrashAt", at)
            result.put("lastCrashVersion", version)
            result.put("lastCrashError", error)
        }
        invoke.resolve(result)
    }

    @Command
    fun openSettings(invoke: Invoke) {
        val args = invoke.parseArgs(KindArgs::class.java)
        activity.runOnUiThread { Permissions.open(activity, args.kind) }
        invoke.resolve()
    }

    @Command
    fun setOemDone(invoke: Invoke) {
        val args = invoke.parseArgs(DoneArgs::class.java)
        Store.get(context).setOemDone(args.done)
        invoke.resolve()
    }

    @Command
    fun secretGet(invoke: Invoke) {
        val args = invoke.parseArgs(NameArgs::class.java)
        // "None" would make Rust start a new local database: refuse instead.
        if (!Secrets.available(context)) {
            invoke.reject("secret store locked")
            return
        }
        val result = JSObject()
        result.put("value", Secrets.get(context, args.name))
        invoke.resolve(result)
    }

    @Command
    fun secretSet(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        try {
            Secrets.put(context, args.name, args.value)
            invoke.resolve()
        } catch (e: Exception) {
            // The message never contains the value.
            invoke.reject("secret store failed: ${e.javaClass.simpleName}")
        }
    }

    @Command
    fun secretDelete(invoke: Invoke) {
        val args = invoke.parseArgs(NameArgs::class.java)
        try {
            Secrets.delete(context, args.name)
        } catch (e: Exception) {
            invoke.reject("secret store failed: ${e.javaClass.simpleName}")
            return
        }
        if (args.name == Secrets.DEVICE_SECRET) {
            // Unpaired: no more alarms on this device.
            synchronized(Store.LOCK) { Store.get(context).clearPlan() }
            Ring.stop(context)
        }
        invoke.resolve()
    }

    @Command
    fun setServerConfig(invoke: Invoke) {
        val args = invoke.parseArgs(ServerConfigArgs::class.java)
        Store.get(context).setServerConfig(args.url, args.publishableKey)
        invoke.resolve()
    }

    /** The status and navigation bars (and cutout) around the app, in CSS pixels. */
    @Command
    fun insets(invoke: Invoke) {
        val result = JSObject()
        val root = activity.window.decorView
        val insets = ViewCompat.getRootWindowInsets(root)
            ?.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
        val density = activity.resources.displayMetrics.density
        result.put("top", (insets?.top ?: 0) / density)
        result.put("right", (insets?.right ?: 0) / density)
        result.put("bottom", (insets?.bottom ?: 0) / density)
        result.put("left", (insets?.left ?: 0) / density)
        invoke.resolve(result)
    }

    @Command
    fun deviceName(invoke: Invoke) {
        val result = JSObject()
        result.put("name", Permissions.deviceName(context))
        invoke.resolve(result)
    }
}
