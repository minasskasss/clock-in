package io.github.minasskasss.clockin.alarm

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
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
class SetPlanArgs {
    lateinit var items: Array<ItemArg>
    lateinit var alertMode: String
    var configVersion: Long = 0
    var horizonEnd: String? = null
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

    /** The plan Rust computed, and this device's alert mode. */
    @Command
    fun setPlan(invoke: Invoke) {
        val args = invoke.parseArgs(SetPlanArgs::class.java)
        val items = args.items.mapNotNull { PlanItem.of(it.itemId, it.firesAt, it.kind, it.name) }
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            store.setAlertMode(args.alertMode)
            store.setPlan(Plan(args.configVersion, args.horizonEnd?.let { PlanItem.parseInstantMs(it) }, items))
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
        Permissions.status(context).forEach { (k, v) -> result.put(k, v) }
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

    @Command
    fun deviceName(invoke: Invoke) {
        val result = JSObject()
        result.put("name", Permissions.deviceName(context))
        invoke.resolve(result)
    }
}
