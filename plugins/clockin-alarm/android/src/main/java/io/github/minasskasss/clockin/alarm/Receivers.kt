package io.github.minasskasss.clockin.alarm

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * The pre-alarm server check (ARCHITECTURE §10, step 2): which of these
 * alarms are still due. Offline or slow (3 s) means all of them: fail loud.
 */
internal object Checker {
    private const val TIMEOUT_MS = 3_000L

    fun stillDue(context: Context, items: List<PlanItem>): List<PlanItem> {
        if (items.isEmpty()) return items
        val store = Store.get(context)
        val api = store.serverApi() ?: return items
        // No secret before the first unlock after a reboot: ring without asking.
        val secret = Secrets.get(context, Secrets.DEVICE_SECRET) ?: return items
        return when (val answer = api.checkAlarm(secret, items, TIMEOUT_MS)) {
            is ServerApi.Answer.Ok -> {
                val list = answer.body.optJSONArray("items")
                val due = mutableMapOf<String, Boolean>()
                if (list != null) {
                    for (i in 0 until list.length()) {
                        val o = list.getJSONObject(i)
                        due[o.optString("item_id")] = o.optBoolean("due", false)
                    }
                }
                // The schedule changed since this plan was made: refresh it.
                if (answer.body.optLong("config_version", 0) > store.plan().configVersion) {
                    PlanWorker.refreshSoon(context)
                }
                items.filter { due[it.itemId] == true }
            }
            is ServerApi.Answer.Rejected ->
                if (answer.unpaired) {
                    // This device was revoked: no more alarms until it is paired again.
                    synchronized(Store.LOCK) { store.clearPlan() }
                    emptyList()
                } else {
                    items
                }
            ServerApi.Answer.Unreachable -> items
        }
    }
}

/** An exact alarm or a re-ring fired. */
class AlarmReceiver : BroadcastReceiver() {
    companion object {
        const val ACTION_FIRE = "io.github.minasskasss.clockin.alarm.FIRE"
        const val ACTION_RERING = "io.github.minasskasss.clockin.alarm.RERING"
        const val EXTRA_AT = "at"
    }

    override fun onReceive(context: Context, intent: Intent) {
        val at = intent.getLongExtra(EXTRA_AT, 0)
        when (intent.action) {
            ACTION_RERING -> RingService.start(context, RingService.ACTION_RERING, at)
            ACTION_FIRE -> {
                if (Store.get(context).alertMode() == Store.MODE_RING) {
                    // Started at once: an exact alarm may start a foreground
                    // service from the background; the service does the check.
                    RingService.start(context, RingService.ACTION_FIRE, at)
                } else {
                    val pending = goAsync()
                    Thread {
                        try {
                            notifyOnce(context, at)
                        } finally {
                            pending.finish()
                        }
                    }.start()
                }
            }
        }
    }

    /** Notification mode (SPEC §7.4): one notification, no service, no repeats. */
    private fun notifyOnce(context: Context, at: Long) {
        val store = Store.get(context)
        val candidates = synchronized(Store.LOCK) {
            val handled = store.handledKeys()
            store.plan().items.filter { it.firesAtMs == at && it.key !in handled }
        }
        val due = Checker.stillDue(context, candidates)
        if (due.isNotEmpty()) {
            store.recordAlarm(System.currentTimeMillis(), "notificationMode")
            Notifications.showReminder(context, due, at / 60_000L)
        }
        synchronized(Store.LOCK) { store.markHandled(candidates, System.currentTimeMillis()) }
        AlarmScheduler.reschedule(context)
    }
}

/**
 * «Σταμάτημα» on the ringing or silent notification, and the same
 * notifications swiped away (they are posted again while the cycle lasts).
 */
class StopReceiver : BroadcastReceiver() {
    companion object {
        const val ACTION_STOP = "io.github.minasskasss.clockin.alarm.STOP"
        const val ACTION_REPOST = "io.github.minasskasss.clockin.alarm.REPOST"
    }

    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            ACTION_STOP -> Ring.stop(context)
            ACTION_REPOST -> Ring.repost(context)
        }
    }
}

/**
 * Boot, app update, clock and permission changes: reschedule only. Never
 * starts a foreground service (Android 15 forbids several types from boot).
 */
class SystemReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        AlarmScheduler.reschedule(context)
        // WorkManager keeps its data in credential-protected storage, which
        // isn't readable before the first unlock after a reboot.
        if (intent.action != Intent.ACTION_LOCKED_BOOT_COMPLETED) {
            PlanWorker.ensurePeriodic(context)
        }
        if (intent.action == Intent.ACTION_MY_PACKAGE_REPLACED) {
            // Android 14+ installers may switch full-screen alarms off on
            // every update: say so at once, not only when the app is opened.
            // A few seconds' wait lets the installer's change land first.
            val pending = goAsync()
            Thread {
                try {
                    Thread.sleep(3_000)
                    if (!Permissions.allGranted(context)) Notifications.showSetupNeeded(context)
                } finally {
                    pending.finish()
                }
            }.start()
        }
    }
}
