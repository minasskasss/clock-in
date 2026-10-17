package io.github.minasskasss.clockin.alarm

import android.app.AlarmManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import androidx.core.content.ContextCompat
import java.util.concurrent.Executors

/**
 * The reminder arithmetic (SPEC §7.6), the same rules as
 * `clockin-core::reminder_at` / `reminders_due`: reminder `k` (1, 2, …) of a
 * group is due at Stop + k·N, in real milliseconds. Every instant it is
 * compared with (the business-day ends) comes from Rust.
 */
internal object ReminderMath {
    /** How many reminders of a group have come due by `nowMs` (0 before the first). */
    fun due(startedAtMs: Long, delayMs: Long, nowMs: Long): Int {
        if (delayMs <= 0 || nowMs <= startedAtMs) return 0
        return ((nowMs - startedAtMs) / delayMs).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
    }

    /** When reminder `k` is due. */
    fun at(startedAtMs: Long, delayMs: Long, k: Int): Long = startedAtMs + delayMs * k
}

/**
 * «Λείπει σήμανση» on Android (SPEC §7.6, ARCHITECTURE §10): after
 * «Σταμάτημα» (or, in notification mode, after the alarm's notification),
 * the names still not marked come back N minutes later, then every N
 * minutes, until «Εντάξει», until everyone is marked, or until their
 * business day ends. Each reminder is an exact alarm (`setAlarmClock`, so it
 * fires in Doze: `setExactAndAllowWhileIdle` may fire only once per 9
 * minutes) that starts [ReminderService], which checks with the server
 * first. Nothing runs between reminders. The alarms themselves are untouched:
 * reminders have their own action and request code, and wait while an alarm
 * rings or is silent between rings.
 */
internal object Reminders {
    const val ACTION_FIRE = "io.github.minasskasss.clockin.alarm.REMINDER_FIRE"
    const val ACTION_OK = "io.github.minasskasss.clockin.alarm.REMINDER_OK"
    const val EXTRA_GROUPS = "groups"

    /** The reminder's request code (alarm codes are epoch minutes; the re-ring is 1). */
    private const val CODE = 2

    /** The delay, or null when the setting is Off. */
    fun delayMs(store: Store): Long? = store.reminderMin().takeIf { it in 1..60 }?.let { it * 60_000L }

    /** Names of a group still before the end of their business day. Unknown end: none (quiet). */
    fun live(items: List<PlanItem>, dayEnds: Map<String, Long>, nowMs: Long): List<PlanItem> =
        items.filter { item -> dayEnds[item.businessDate]?.let { nowMs < it } == true }

    /** An alarm is ringing or silent between rings: reminders wait for it. */
    private fun held(store: Store): Boolean = store.active() != null

    /** «Σταμάτημα» ended these alarms, or notification mode posted them, at `atMs`. */
    fun start(context: Context, items: List<PlanItem>, atMs: Long) {
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            if (delayMs(store) == null) return
            val markable = items.filter { it.markable }.distinctBy { it.key }
            if (markable.isEmpty()) return
            store.setReminders(store.reminders() + ReminderGroup(store.nextReminderId(), markable, atMs, 0))
        }
        schedule(context)
    }

    /**
     * The groups showing now, and their names still due (plan order). With
     * `whileAlarm` false, nothing while an alarm is in progress (the Today
     * bar); the notification stays in the shade meanwhile.
     */
    fun showing(
        context: Context,
        nowMs: Long = System.currentTimeMillis(),
        whileAlarm: Boolean = true,
    ): Pair<List<Long>, List<PlanItem>> {
        val store = Store.get(context)
        if (!whileAlarm && held(store)) return emptyList<Long>() to emptyList()
        val ends = store.dayEnds()
        val groups = store.reminders().filter { it.shown >= 1 && live(it.items, ends, nowMs).isNotEmpty() }
        val items = groups.flatMap { live(it.items, ends, nowMs) }
            .distinctBy { it.key }
            .sortedWith(compareBy<PlanItem>({ it.firesAtMs }, { it.kind != "in" }, { it.name }))
        return groups.map { it.id } to items
    }

    /**
     * When the showing reminder comes back, or null when this is the last
     * one before its business day ends (or nothing shows).
     */
    fun nextShown(context: Context, nowMs: Long = System.currentTimeMillis()): Long? {
        val store = Store.get(context)
        val delay = delayMs(store) ?: return null
        val ends = store.dayEnds()
        return store.reminders()
            .filter { it.shown >= 1 }
            .mapNotNull { g ->
                val last = live(g.items, ends, nowMs).mapNotNull { ends[it.businessDate] }.maxOrNull() ?: return@mapNotNull null
                ReminderMath.at(g.startedAtMs, delay, g.shown + 1).takeIf { it < last }
            }
            .minOrNull()
    }

    /** «Εντάξει»: these groups end on this phone. It marks nobody. */
    fun ok(context: Context, ids: Collection<Long>) {
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            store.setReminders(store.reminders().filter { it.id !in ids })
        }
        afterChange(context)
    }

    /** These alarms are no longer due (marked, moved, removed): their names go. */
    fun drop(context: Context, keys: Set<String>) {
        if (keys.isEmpty()) return
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            val before = store.reminders()
            val after = before.map { g -> g.copy(items = g.items.filter { it.key !in keys }) }
            if (after == before) return
            store.setReminders(after)
        }
        afterChange(context)
    }

    /**
     * Rust sent a new plan (it leaves out alarms that marks known on this
     * phone suppress, and it reaches back to the previous business day):
     * names whose alarm is not in it at the same time are not due any more.
     * Not for the server's plan, which keeps marked alarms.
     */
    fun onPlanChanged(context: Context, plan: List<PlanItem>) {
        val keys = plan.map { it.key }.toSet()
        val gone = Store.get(context).reminders().flatMap { it.items }.map { it.key }.filter { it !in keys }.toSet()
        drop(context, gone)
    }

    /** Unpaired: no reminders. */
    fun clear(context: Context) {
        synchronized(Store.LOCK) { Store.get(context).setReminders(emptyList()) }
        Notifications.cancel(context, Notifications.ID_REMINDER)
        cancelAlarm(context)
    }

    private fun afterChange(context: Context) {
        Notifications.showUnmarked(context, alert = false)
        schedule(context)
        Ring.changed()
    }

    /** The «Σήμανση» names the Today bar shows: group ids and names. */
    fun status(context: Context): Pair<List<Long>, List<String>> {
        val (ids, items) = showing(context, whileAlarm = false)
        return ids to items.map { it.name }.distinct()
    }

    private fun alarmIntent(context: Context, flags: Int): PendingIntent? =
        PendingIntent.getBroadcast(
            context,
            CODE,
            Intent(context, AlarmReceiver::class.java).setAction(ACTION_FIRE),
            flags or PendingIntent.FLAG_IMMUTABLE,
        )

    private fun cancelAlarm(context: Context) {
        val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        alarmIntent(context, PendingIntent.FLAG_NO_CREATE)?.let {
            am.cancel(it)
            it.cancel()
        }
    }

    /**
     * Schedules the next reminder, or the moment a showing one must close
     * (its business day ends); drops what can't remind any more. Nothing
     * while an alarm is in progress: its end reschedules.
     */
    fun schedule(context: Context) {
        val now = System.currentTimeMillis()
        val next: Long?
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            val delay = delayMs(store)
            if (delay == null) {
                if (store.reminders().isNotEmpty()) store.setReminders(emptyList())
                next = null
            } else {
                val ends = store.dayEnds()
                val groups = store.reminders()
                val kept = groups.filter { live(it.items, ends, now).isNotEmpty() }
                if (kept.size != groups.size) store.setReminders(kept)
                next = if (held(store)) {
                    null
                } else {
                    kept.flatMap { g ->
                        val liveEnds = live(g.items, ends, now).mapNotNull { ends[it.businessDate] }
                        val last = liveEnds.maxOrNull() ?: return@flatMap emptyList<Long>()
                        // A reminder missed while the phone was off, or while an
                        // alarm rang, comes at once.
                        val k = maxOf(g.shown, 0) + 1
                        val due = ReminderMath.at(g.startedAtMs, delay, k)
                        listOfNotNull(
                            due.takeIf { it < last },
                            liveEnds.minOrNull().takeIf { g.shown >= 1 },
                        )
                    }.minOrNull()
                }
            }
        }
        cancelAlarm(context)
        if (next == null) {
            if (showing(context).second.isEmpty()) Notifications.cancel(context, Notifications.ID_REMINDER)
            return
        }
        val operation = alarmIntent(context, PendingIntent.FLAG_UPDATE_CURRENT) ?: return
        val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        val at = maxOf(next, now + 1_000)
        if (AlarmScheduler.canScheduleExact(context)) {
            val show = Notifications.openAppPendingIntent(context) ?: operation
            am.setAlarmClock(AlarmManager.AlarmClockInfo(at, show), operation)
        } else {
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, operation)
        }
    }
}

/**
 * Shows one «Λείπει σήμανση» reminder (SPEC §7.6): asks the server which
 * names are still due (offline, slow or before the first unlock: all of
 * them, fail loud), posts or updates the reminder (full screen in ring mode,
 * an ordinary notification in notification mode), plays the C once unless
 * Do Not Disturb is on, schedules the next one and stops. Started only from
 * the reminder's exact alarm (an allowed background start), with the same
 * foreground-service types as [RingService].
 */
class ReminderService : Service() {
    companion object {
        fun start(context: Context) {
            try {
                ContextCompat.startForegroundService(context, Intent(context, ReminderService::class.java))
            } catch (e: Exception) {
                // Refused (should not happen after an exact alarm): show it
                // from the stored data, unchecked and silent.
                Notifications.showUnmarked(context, alert = true)
                Reminders.schedule(context)
            }
        }

        /** The sound never lasts longer; the service stops by then at the latest. */
        private const val MAX_RUN_MS = 8_000L
    }

    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private var player: MediaPlayer? = null
    private val stop = Runnable { finish() }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        Notifications.ensureChannels(this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val inForeground = try {
            val notification = Notifications.checking(this)
            when {
                Build.VERSION.SDK_INT >= 34 ->
                    startForeground(Notifications.ID_CHECKING_REMINDER, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED)
                Build.VERSION.SDK_INT >= 29 ->
                    startForeground(Notifications.ID_CHECKING_REMINDER, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
                else -> startForeground(Notifications.ID_CHECKING_REMINDER, notification)
            }
            true
        } catch (e: Exception) {
            false
        }
        if (!inForeground) {
            Notifications.showUnmarked(this, alert = true)
            Reminders.schedule(this)
            stopSelf()
            return START_NOT_STICKY
        }
        main.removeCallbacks(stop)
        main.postDelayed(stop, MAX_RUN_MS)
        worker.execute { remind() }
        return START_NOT_STICKY
    }

    /** Worker thread. */
    private fun remind() {
        val store = Store.get(this)
        val now = System.currentTimeMillis()
        val delay = Reminders.delayMs(store)
        if (delay == null || store.active() != null) {
            Reminders.schedule(this)
            main.post { finish() }
            return
        }
        val ends = store.dayEnds()
        val due = store.reminders().filter { ReminderMath.due(it.startedAtMs, delay, now) >= 1 }
        val asked = due.flatMap { Reminders.live(it.items, ends, now) }.distinctBy { it.key }
        val stillDue = Checker.stillDue(this, asked).map { it.key }.toSet()
        val notDue = asked.map { it.key }.filter { it !in stillDue }.toSet()
        var chime = false
        synchronized(Store.LOCK) {
            val dueIds = due.map { it.id }.toSet()
            val groups = store.reminders().map { g ->
                val items = g.items.filter { it.key !in notDue }
                val k = ReminderMath.due(g.startedAtMs, delay, now)
                if (g.id in dueIds && k > g.shown && Reminders.live(items, ends, now).isNotEmpty()) {
                    chime = true
                    g.copy(items = items, shown = k)
                } else {
                    g.copy(items = items)
                }
            }
            store.setReminders(groups)
        }
        Notifications.showUnmarked(this, alert = chime)
        Reminders.schedule(this)
        Ring.changed()
        main.post {
            if (chime && !Notifications.doNotDisturb(this)) {
                playChime(ring = store.alertMode() == Store.MODE_RING)
            } else {
                finish()
            }
        }
    }

    /**
     * The C once. Ring mode on the alarm stream (it isn't muted by the
     * ringer switch), notification mode on the notification stream; never
     * under Do Not Disturb (checked by the caller).
     */
    private fun playChime(ring: Boolean) {
        try {
            val afd = resources.openRawResourceFd(R.raw.reminder)
            player = MediaPlayer().apply {
                setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(if (ring) AudioAttributes.USAGE_ALARM else AudioAttributes.USAGE_NOTIFICATION_EVENT)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build(),
                )
                setDataSource(afd.fileDescriptor, afd.startOffset, afd.length)
                setOnCompletionListener { finish() }
                prepare()
                start()
            }
            afd.close()
        } catch (e: Exception) {
            finish()
        }
    }

    private fun finish() {
        main.removeCallbacks(stop)
        player?.let {
            try {
                it.stop()
            } catch (e: Exception) {
            }
            it.release()
        }
        player = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        Notifications.cancel(this, Notifications.ID_CHECKING_REMINDER)
        stopSelf()
    }

    override fun onDestroy() {
        main.removeCallbacks(stop)
        player?.release()
        player = null
        worker.shutdown()
        super.onDestroy()
    }
}
