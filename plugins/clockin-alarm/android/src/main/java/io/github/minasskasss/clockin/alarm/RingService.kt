package io.github.minasskasss.clockin.alarm

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
import android.os.PowerManager
import android.os.VibrationEffect
import android.os.Vibrator
import androidx.core.content.ContextCompat
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors

/** Shared ring-mode actions (SPEC §7.3) and change listeners for the alarm screen. */
internal object Ring {
    private val main = Handler(Looper.getMainLooper())
    private val listeners = CopyOnWriteArrayList<() -> Unit>()

    private val greekTime: DateTimeFormatter =
        DateTimeFormatter.ofPattern("HH:mm").withZone(ZoneId.of("Europe/Athens"))

    /** "HH:MM", Greek time whatever the phone's timezone (SPEC §4.2). */
    fun hhmm(ms: Long): String = greekTime.format(Instant.ofEpochMilli(ms))

    fun addListener(listener: () -> Unit) = listeners.add(listener)

    fun removeListener(listener: () -> Unit) = listeners.remove(listener)

    fun changed() {
        main.post { listeners.forEach { it() } }
    }

    /**
     * «Σταμάτημα»: ends this alarm cycle on this device only. It marks
     * nobody (SPEC §7.3).
     */
    fun stop(context: Context) {
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            store.active()?.let { store.markHandled(it.items, System.currentTimeMillis()) }
            store.setActive(null)
        }
        Notifications.cancel(context, Notifications.ID_SILENT)
        Notifications.cancel(context, Notifications.ID_RINGING)
        RingService.running?.end()
        AlarmScheduler.reschedule(context)
        changed()
    }

    /**
     * The ringing or silent notification was swiped away (Android 14+ allows
     * that unless the phone is locked): post it again while the cycle lasts,
     * so «Σταμάτημα» is always reachable.
     */
    fun repost(context: Context) {
        val service = RingService.running
        if (service != null && service.isRinging) {
            service.refreshNotification()
            return
        }
        val active = Store.get(context).active() ?: return
        val rering = active.reringAtMs ?: return
        Notifications.showSilent(context, active.items, hhmm(rering))
    }

    /**
     * A cycle can start before notifications are allowed (an alarm up to 15
     * minutes late right after pairing, Android 13+): Android then plays the
     * sound but shows nothing. Once they are allowed, post it so «Σταμάτημα»
     * is reachable. Called with the permission check and while ringing.
     */
    fun ensureShown(context: Context) {
        if (!Permissions.notificationsAllowed(context)) return
        val active = Store.get(context).active() ?: return
        val id = if (RingService.running?.isRinging == true) Notifications.ID_RINGING else Notifications.ID_SILENT
        if (id == Notifications.ID_SILENT && active.reringAtMs == null) return
        if (!Notifications.isShowing(context, id)) repost(context)
    }

    /**
     * The plan changed (a mark on this phone, a schedule edit): names whose
     * alarm is no longer in the plan at the same time are dropped at once,
     * and the cycle ends when nobody is left.
     */
    fun onPlanChanged(context: Context) {
        val ended: Boolean
        synchronized(Store.LOCK) {
            val store = Store.get(context)
            val active = store.active() ?: return
            val keys = store.plan().items.map { it.key }.toSet()
            val remaining = active.items.filter { it.key in keys }
            if (remaining.size == active.items.size) return
            ended = remaining.isEmpty()
            if (ended) {
                store.markHandled(active.items, System.currentTimeMillis())
                store.setActive(null)
            } else {
                store.setActive(active.copy(items = remaining))
            }
        }
        if (ended) {
            Notifications.cancel(context, Notifications.ID_SILENT)
            Notifications.cancel(context, Notifications.ID_RINGING)
            RingService.running?.end()
        } else {
            RingService.running?.refreshNotification()
        }
        changed()
    }
}

/**
 * Rings one alarm cycle (ARCHITECTURE §10): checks with the server, loops
 * the sound on the alarm stream for 5 minutes with a full-screen
 * notification, then schedules the re-ring 5 minutes later and stops.
 *
 * Foreground-service type: `systemExempted` on Android 14+ (allowed for apps
 * holding an exact-alarm permission), `mediaPlayback` on Android 10–13.
 */
class RingService : Service() {
    companion object {
        const val ACTION_FIRE = "io.github.minasskasss.clockin.alarm.RING_FIRE"
        const val ACTION_RERING = "io.github.minasskasss.clockin.alarm.RING_RERING"
        private const val EXTRA_AT = "at"

        @Volatile
        internal var running: RingService? = null
            private set

        fun start(context: Context, action: String, at: Long) {
            val intent = Intent(context, RingService::class.java).setAction(action).putExtra(EXTRA_AT, at)
            try {
                ContextCompat.startForegroundService(context, intent)
            } catch (e: Exception) {
                // The system refused the service (should not happen after an
                // exact alarm): still alert with a plain notification.
                val items = Store.get(context).plan().items.filter { it.firesAtMs == at }
                if (items.isNotEmpty()) Notifications.showReminder(context, items, at / 60_000L)
            }
        }
    }

    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private var player: MediaPlayer? = null
    private var vibrating = false
    private var wakeLock: PowerManager.WakeLock? = null
    /** Written on the main thread only. */
    @Volatile
    private var ringing = false

    internal val isRinging: Boolean get() = ringing
    private val silence = Runnable { goSilent() }
    private val recheckTick = Runnable { worker.execute { recheck() } }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        running = this
        Notifications.ensureChannels(this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Must be in the foreground within seconds of being started.
        val active = Store.get(this).active()
        val inForeground = if (ringing && active != null) {
            enterForeground(Notifications.ID_RINGING, Notifications.ringing(this, active.items))
        } else {
            enterForeground(Notifications.ID_CHECKING, Notifications.checking(this))
        }
        val at = intent?.getLongExtra(EXTRA_AT, 0) ?: 0
        if (!inForeground) {
            // Android refused the foreground service: never lose the alarm,
            // alert with a plain notification instead.
            val items = Store.get(this).plan().items.filter { it.firesAtMs == at }
            if (intent?.action == ACTION_FIRE && items.isNotEmpty()) Notifications.showReminder(this, items, at / 60_000L)
            stopSelf()
            return START_NOT_STICKY
        }
        when (intent?.action) {
            ACTION_FIRE -> worker.execute { fire(at) }
            ACTION_RERING -> worker.execute { rering() }
            else -> main.post { if (!ringing) finish() }
        }
        return START_NOT_STICKY
    }

    private fun enterForeground(id: Int, notification: android.app.Notification): Boolean =
        try {
            when {
                Build.VERSION.SDK_INT >= 34 ->
                    startForeground(id, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED)
                Build.VERSION.SDK_INT >= 29 ->
                    startForeground(id, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
                else -> startForeground(id, notification)
            }
            true
        } catch (e: Exception) {
            false
        }

    /** An alarm event's minute arrived (worker thread). */
    private fun fire(at: Long) {
        val store = Store.get(this)
        val candidates = synchronized(Store.LOCK) {
            val handled = store.handledKeys()
            val activeKeys = store.active()?.items?.map { it.key }?.toSet() ?: emptySet()
            store.plan().items.filter { it.firesAtMs == at && it.key !in handled && it.key !in activeKeys }
        }
        val due = Checker.stillDue(this, candidates)
        val dueKeys = due.map { it.key }.toSet()
        synchronized(Store.LOCK) {
            val now = System.currentTimeMillis()
            // Checked and not due (marked, moved, removed): never ring them.
            store.markHandled(candidates.filter { it.key !in dueKeys }, now)
            if (due.isNotEmpty()) {
                // New names join a cycle in progress, and the ring starts again.
                val current = store.active()?.items ?: emptyList()
                store.setActive(Active(current + due, now, null))
            }
        }
        AlarmScheduler.reschedule(this)
        main.post { if (due.isNotEmpty()) startRinging() else if (!ringing) finish() }
    }

    /** The silent 5 minutes are over (worker thread). */
    private fun rering() {
        val store = Store.get(this)
        val active = store.active()
        if (active == null) {
            main.post { if (!ringing) finish() }
            return
        }
        // Names marked or moved meanwhile are dropped (SPEC §7.3, §7.2).
        val keys = store.plan().items.map { it.key }.toSet()
        val due = Checker.stillDue(this, active.items.filter { it.key in keys })
        synchronized(Store.LOCK) {
            val now = System.currentTimeMillis()
            if (due.isEmpty()) {
                store.markHandled(active.items, now)
                store.setActive(null)
            } else {
                store.setActive(Active(due, now, null))
            }
        }
        Notifications.cancel(this, Notifications.ID_SILENT)
        AlarmScheduler.reschedule(this)
        main.post {
            if (due.isNotEmpty()) {
                startRinging()
            } else {
                if (!ringing) finish()
                Ring.changed()
            }
        }
    }

    private fun startRinging() {
        val active = Store.get(this).active()
        if (active == null) {
            if (!ringing) finish()
            return
        }
        ringing = true
        // "Notification" until the alarm screen reports how it was shown.
        Store.get(this).recordAlarm(System.currentTimeMillis(), "notification")
        enterForeground(Notifications.ID_RINGING, Notifications.ringing(this, active.items))
        Notifications.cancel(this, Notifications.ID_CHECKING)
        Notifications.cancel(this, Notifications.ID_SILENT)
        // An event with any check-in plays the check-in sound (SPEC §7.1).
        startSound(active.items.any { it.kind == "in" })
        acquireWakeLock()
        main.removeCallbacks(silence)
        main.postDelayed(silence, AlarmScheduler.RING_MS)
        main.removeCallbacks(recheckTick)
        main.postDelayed(recheckTick, AlarmScheduler.RECHECK_MS)
        Ring.changed()
    }

    /**
     * While ringing (worker thread): asks the server again, drops names
     * marked, moved or removed elsewhere, and ends when nobody is left. A
     * failed check (offline, slow, or no secret before the first unlock)
     * keeps every name ringing (fail loud).
     */
    private fun recheck() {
        val store = Store.get(this)
        val asked = store.active()?.items ?: return
        val dueKeys = Checker.stillDue(this, asked).map { it.key }.toSet()
        val askedKeys = asked.map { it.key }.toSet()
        val ended: Boolean
        val changed: Boolean
        synchronized(Store.LOCK) {
            // The cycle may have changed during the check (Stop, new names joining).
            val active = store.active()
            if (active == null) {
                ended = false
                changed = false
            } else {
                val dropped = active.items.filter { it.key in askedKeys && it.key !in dueKeys }
                val remaining = active.items - dropped.toSet()
                changed = dropped.isNotEmpty()
                ended = remaining.isEmpty()
                if (ended) {
                    store.markHandled(active.items, System.currentTimeMillis())
                    store.setActive(null)
                } else if (changed) {
                    store.markHandled(dropped, System.currentTimeMillis())
                    store.setActive(active.copy(items = remaining))
                }
            }
        }
        if (ended) {
            Notifications.cancel(this, Notifications.ID_SILENT)
            Notifications.cancel(this, Notifications.ID_RINGING)
            AlarmScheduler.reschedule(this)
            end()
            Ring.changed()
            return
        }
        if (changed) {
            refreshNotification()
            Ring.changed()
        }
        Ring.ensureShown(this)
        main.post {
            if (ringing) {
                main.removeCallbacks(recheckTick)
                main.postDelayed(recheckTick, AlarmScheduler.RECHECK_MS)
            }
        }
    }

    private fun goSilent() {
        stopSound()
        ringing = false
        main.removeCallbacks(recheckTick)
        val reringAt = System.currentTimeMillis() + AlarmScheduler.SILENT_MS
        val active = synchronized(Store.LOCK) {
            val store = Store.get(this)
            store.active()?.copy(reringAtMs = reringAt)?.also { store.setActive(it) }
        }
        if (active != null) {
            AlarmScheduler.reschedule(this)
            Notifications.showSilent(this, active.items, Ring.hhmm(reringAt))
        }
        finish()
        Ring.changed()
    }

    /** Updates the names on the ringing notification. */
    internal fun refreshNotification() {
        main.post {
            val active = Store.get(this).active() ?: return@post
            if (ringing) enterForeground(Notifications.ID_RINGING, Notifications.ringing(this, active.items))
        }
    }

    /** Stop pressed, or nobody is left: silence and go. */
    internal fun end() {
        main.post {
            stopSound()
            ringing = false
            main.removeCallbacks(silence)
            main.removeCallbacks(recheckTick)
            finish()
        }
    }

    private fun finish() {
        stopForeground(STOP_FOREGROUND_REMOVE)
        Notifications.cancel(this, Notifications.ID_CHECKING)
        releaseWakeLock()
        stopSelf()
    }

    private fun startSound(checkIn: Boolean) {
        stopSound()
        try {
            val afd = resources.openRawResourceFd(if (checkIn) R.raw.check_in else R.raw.check_out)
            player = MediaPlayer().apply {
                setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_ALARM)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build(),
                )
                setDataSource(afd.fileDescriptor, afd.startOffset, afd.length)
                isLooping = true
                prepare()
                start()
            }
            afd.close()
        } catch (e: Exception) {
            player = null
        }
        @Suppress("DEPRECATION")
        val vibrator = getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        if (vibrator?.hasVibrator() == true) {
            vibrator.vibrate(
                VibrationEffect.createWaveform(longArrayOf(0, 700, 900), 0),
                AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ALARM).build(),
            )
            vibrating = true
        }
    }

    private fun stopSound() {
        player?.let {
            try {
                it.stop()
            } catch (e: Exception) {
            }
            it.release()
        }
        player = null
        if (vibrating) {
            @Suppress("DEPRECATION")
            (getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator)?.cancel()
            vibrating = false
        }
    }

    private fun acquireWakeLock() {
        if (wakeLock?.isHeld == true) return
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "ClockIn:ring").apply {
            setReferenceCounted(false)
            acquire(AlarmScheduler.RING_MS + 60_000)
        }
    }

    private fun releaseWakeLock() {
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
    }

    override fun onDestroy() {
        main.removeCallbacks(silence)
        main.removeCallbacks(recheckTick)
        stopSound()
        releaseWakeLock()
        worker.shutdown()
        if (running === this) running = null
        super.onDestroy()
    }
}
