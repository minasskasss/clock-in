package io.github.minasskasss.clockin.alarm

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build

/**
 * Turns the stored plan into exact alarms (ARCHITECTURE §10). Kotlin never
 * computes schedules: every time comes from the plan Rust made. It only
 * compares instants and adds the fixed spans below (DECISIONS, Phase 5).
 */
internal object AlarmScheduler {
    /** Schedule the plan's alarms this far ahead; later ones on a later reschedule. */
    const val AHEAD_MS = 48L * 60 * 60 * 1000
    /** A missed alarm still fires up to 15 minutes late (SPEC §7.5). */
    const val GRACE_MS = 15L * 60 * 1000
    /** Ring 5 minutes, then silent 5 minutes (SPEC §7.3). */
    const val RING_MS = 5L * 60 * 1000
    const val SILENT_MS = 5L * 60 * 1000
    /** While ringing, ask the server again this often (marks made elsewhere). */
    const val RECHECK_MS = 25L * 1000

    /** The request code of the re-ring alarm (alarm codes are epoch minutes, far above). */
    private const val RERING_CODE = 1

    private fun alarmManager(context: Context) =
        context.getSystemService(Context.ALARM_SERVICE) as AlarmManager

    fun canScheduleExact(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarmManager(context).canScheduleExactAlarms()

    private fun intent(context: Context, action: String, atMs: Long) =
        Intent(context, AlarmReceiver::class.java).setAction(action).putExtra(AlarmReceiver.EXTRA_AT, atMs)

    private fun pending(context: Context, code: Int, intent: Intent, flags: Int): PendingIntent? =
        PendingIntent.getBroadcast(context, code, intent, flags or PendingIntent.FLAG_IMMUTABLE)

    private fun set(context: Context, atMs: Long, operation: PendingIntent) {
        val am = alarmManager(context)
        if (canScheduleExact(context)) {
            // Exact, wakes the phone even in Doze, and shows the next-alarm icon.
            val show = Notifications.openAppPendingIntent(context) ?: operation
            am.setAlarmClock(AlarmManager.AlarmClockInfo(atMs, show), operation)
        } else {
            // No exact-alarm permission: late is better than never; the
            // checklist and the Today banner say what is missing.
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMs, operation)
        }
    }

    /** The next alarm or re-ring this device will ring for, if any (diagnostics). */
    fun nextAt(context: Context): Long? {
        val store = Store.get(context)
        val now = System.currentTimeMillis()
        val handled = store.handledKeys()
        val next = store.plan().items
            .filter { it.firesAtMs >= now && it.key !in handled }
            .minOfOrNull { it.firesAtMs }
        val rering = store.active()?.reringAtMs
        return listOfNotNull(next, rering).minOrNull()
    }

    /**
     * Cancels the alarms scheduled last time and schedules every plan event
     * in the next 48 hours that this device hasn't handled, plus the re-ring
     * of a silent alarm. An event missed by up to 15 minutes fires at once.
     */
    fun reschedule(context: Context) {
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            val am = alarmManager(context)
            for (code in store.scheduledCodes()) {
                val old = pending(context, code, intent(context, AlarmReceiver.ACTION_FIRE, 0), PendingIntent.FLAG_NO_CREATE)
                if (old != null) {
                    am.cancel(old)
                    old.cancel()
                }
            }
            pending(context, RERING_CODE, intent(context, AlarmReceiver.ACTION_RERING, 0), PendingIntent.FLAG_NO_CREATE)?.let {
                am.cancel(it)
                it.cancel()
            }

            val now = System.currentTimeMillis()
            val handled = store.handledKeys()
            val active = store.active()
            val activeKeys = active?.items?.map { it.key }?.toSet() ?: emptySet()
            val events = store.plan().items
                .filter { it.firesAtMs >= now - GRACE_MS && it.firesAtMs < now + AHEAD_MS }
                .filter { it.key !in handled && it.key !in activeKeys }
                .map { it.firesAtMs }
                .distinct()
                .sorted()

            val codes = mutableListOf<Int>()
            for (at in events) {
                val code = (at / 60_000L).toInt()
                val operation = pending(context, code, intent(context, AlarmReceiver.ACTION_FIRE, at), PendingIntent.FLAG_UPDATE_CURRENT) ?: continue
                set(context, maxOf(at, now + 1_000), operation)
                codes += code
            }
            store.setScheduledCodes(codes)

            val rering = active?.reringAtMs
            if (rering != null) {
                val operation = pending(context, RERING_CODE, intent(context, AlarmReceiver.ACTION_RERING, rering), PendingIntent.FLAG_UPDATE_CURRENT)
                if (operation != null) set(context, maxOf(rering, now + 1_000), operation)
            }
        }
    }
}
