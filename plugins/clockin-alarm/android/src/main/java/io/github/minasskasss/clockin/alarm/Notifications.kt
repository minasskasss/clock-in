package io.github.minasskasss.clockin.alarm

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat

/**
 * Notification channels (ARCHITECTURE §10) and the notifications the alarm
 * code posts. Every text comes from `src/i18n/el.json` (generated
 * `R.string.clockin_*`).
 */
internal object Notifications {
    const val CHANNEL_ALARM = "alarm"
    const val CHANNEL_REMINDERS = "reminders"
    const val CHANNEL_SERVICE = "service"

    const val ID_CHECKING = 1001
    const val ID_RINGING = 1002
    const val ID_SILENT = 1003
    const val ID_HORIZON = 1004
    const val ID_SETUP = 1005
    /** The «Λείπει σήμανση» reminder (SPEC §7.6). */
    const val ID_REMINDER = 1006
    /** The reminder service's own "checking" notification (apart from the ring's). */
    const val ID_CHECKING_REMINDER = 1007
    /** Notification-mode alarms use their minute as id, above this. */
    private const val ID_NOTIFY_BASE = 2000

    private fun manager(context: Context) =
        context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    fun ensureChannels(context: Context) {
        val m = manager(context)
        // Ring mode: the looping sound comes from the ringing service (alarm
        // stream), so the channel itself is silent; it only vibrates.
        m.createNotificationChannel(
            NotificationChannel(CHANNEL_ALARM, context.getString(R.string.clockin_android_channelAlarm), NotificationManager.IMPORTANCE_HIGH).apply {
                description = context.getString(R.string.clockin_android_channelAlarmDescription)
                setSound(null, null)
                enableVibration(false)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            },
        )
        // Notification mode: the standard notification sound, once.
        m.createNotificationChannel(
            NotificationChannel(CHANNEL_REMINDERS, context.getString(R.string.clockin_android_channelReminders), NotificationManager.IMPORTANCE_HIGH).apply {
                description = context.getString(R.string.clockin_android_channelRemindersDescription)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            },
        )
        m.createNotificationChannel(
            NotificationChannel(CHANNEL_SERVICE, context.getString(R.string.clockin_android_channelService), NotificationManager.IMPORTANCE_LOW).apply {
                description = context.getString(R.string.clockin_android_channelServiceDescription)
                setShowBadge(false)
            },
        )
    }

    /** Whether the user switched off the channel the ring-mode alarm needs. */
    fun alarmChannelBlocked(context: Context): Boolean {
        ensureChannels(context)
        val m = manager(context)
        return !m.areNotificationsEnabled() ||
            m.getNotificationChannel(CHANNEL_ALARM)?.importance == NotificationManager.IMPORTANCE_NONE
    }

    private fun openAppIntent(context: Context): PendingIntent? {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return null
        launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
        return PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    fun openAppPendingIntent(context: Context): PendingIntent? = openAppIntent(context)

    /**
     * The alarm screen, opened by the system (full screen) or by a tap on
     * the notification; `via` tells them apart for the diagnostics.
     */
    private fun alarmScreenIntent(context: Context, fullScreen: Boolean): PendingIntent {
        val intent = Intent(context, AlarmActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_USER_ACTION)
            .putExtra(AlarmActivity.EXTRA_VIA, if (fullScreen) AlarmActivity.VIA_FULL_SCREEN else AlarmActivity.VIA_TAP)
        return PendingIntent.getActivity(context, if (fullScreen) 3 else 1, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    private fun stopIntent(context: Context): PendingIntent {
        val intent = Intent(context, StopReceiver::class.java).setAction(StopReceiver.ACTION_STOP)
        return PendingIntent.getBroadcast(context, 2, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    /** Swiped away (allowed since Android 14): post it again, so «Σταμάτημα» is always there. */
    private fun repostIntent(context: Context): PendingIntent {
        val intent = Intent(context, StopReceiver::class.java).setAction(StopReceiver.ACTION_REPOST)
        return PendingIntent.getBroadcast(context, 4, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    /** The «Λείπει σήμανση» screen, opened by the system (full screen) or by a tap. */
    private fun reminderScreenIntent(context: Context, fullScreen: Boolean): PendingIntent {
        val intent = Intent(context, ReminderActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_USER_ACTION)
        return PendingIntent.getActivity(context, if (fullScreen) 7 else 6, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    /** «Εντάξει» on the reminder notification: ends the groups it showed. */
    private fun reminderOkIntent(context: Context, groupIds: List<Long>): PendingIntent {
        val intent = Intent(context, StopReceiver::class.java)
            .setAction(Reminders.ACTION_OK)
            .putExtra(Reminders.EXTRA_GROUPS, groupIds.toLongArray())
        return PendingIntent.getBroadcast(context, 5, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    /**
     * Do Not Disturb is on (anything but "all interruptions"): the
     * notification-mode alarm and every reminder stay silent.
     */
    fun doNotDisturb(context: Context): Boolean =
        context.getSystemService(NotificationManager::class.java)
            ?.currentInterruptionFilter
            ?.let { it != NotificationManager.INTERRUPTION_FILTER_ALL && it != NotificationManager.INTERRUPTION_FILTER_UNKNOWN }
            ?: false

    /**
     * Posts, updates or removes the «Λείπει σήμανση» reminder (SPEC §7.6):
     * the names still not marked, each with its own time, and «Εντάξει».
     * Ring mode: the full-screen reminder screen on the alarm channel (no
     * sound of its own; the reminder service plays the C). Notification
     * mode: an ordinary notification, always silent here for the same
     * reason. `alert`: a new reminder (full screen again, heads-up); else
     * only the content changes.
     */
    fun showUnmarked(context: Context, alert: Boolean) {
        val (groups, items) = Reminders.showing(context)
        if (items.isEmpty()) {
            cancel(context, ID_REMINDER)
            return
        }
        val next = Reminders.nextShown(context)
        val nextLine = if (next != null) {
            context.getString(R.string.clockin_reminder_next, Ring.hhmm(next))
        } else {
            context.getString(R.string.clockin_reminder_last)
        }
        val text = reminderLines(context, items) + "\n" + nextLine
        val ring = Store.get(context).alertMode() == Store.MODE_RING
        val builder = NotificationCompat.Builder(context, if (ring) CHANNEL_ALARM else CHANNEL_REMINDERS)
            .setSmallIcon(R.drawable.clockin_notification)
            .setContentTitle(context.getString(R.string.clockin_reminder_notificationTitle))
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOnlyAlertOnce(!alert)
            .addAction(0, context.getString(R.string.clockin_reminder_ok), reminderOkIntent(context, groups))
        if (ring) {
            // Like the ringing alarm: never `setSilent`, or SystemUI refuses
            // the full screen; the channel itself has no sound.
            builder
                .setCategory(NotificationCompat.CATEGORY_ALARM)
                .setPriority(NotificationCompat.PRIORITY_MAX)
                .setContentIntent(reminderScreenIntent(context, fullScreen = false))
                .setFullScreenIntent(reminderScreenIntent(context, fullScreen = true), true)
        } else {
            builder
                .setCategory(NotificationCompat.CATEGORY_REMINDER)
                .setPriority(NotificationCompat.PRIORITY_HIGH)
                .setSilent(true)
                .setContentIntent(openAppIntent(context))
        }
        notify(context, ID_REMINDER, builder.build())
    }

    /** "Άφιξη: 18:00  Name, …" lines, every name with its own time. */
    private fun reminderLines(context: Context, items: List<PlanItem>): String {
        val lines = mutableListOf<String>()
        val checkIn = Ring.names(items, "in", withTimes = true)
        val checkOut = Ring.names(items, "out", withTimes = true)
        if (checkIn.isNotEmpty()) lines += context.getString(R.string.clockin_android_checkInLine, checkIn.joinToString(", "))
        if (checkOut.isNotEmpty()) lines += context.getString(R.string.clockin_android_checkOutLine, checkOut.joinToString(", "))
        return lines.joinToString("\n")
    }

    /**
     * "Άφιξη: …" and "Αποχώρηση: …" lines for these alarms; each name with
     * its own time when they are due at different minutes.
     */
    fun nameLines(context: Context, items: List<PlanItem>): String {
        val lines = mutableListOf<String>()
        val withTimes = !Ring.oneMinute(items)
        val checkIn = Ring.names(items, "in", withTimes)
        val checkOut = Ring.names(items, "out", withTimes)
        if (checkIn.isNotEmpty()) lines += context.getString(R.string.clockin_android_checkInLine, checkIn.joinToString(", "))
        if (checkOut.isNotEmpty()) lines += context.getString(R.string.clockin_android_checkOutLine, checkOut.joinToString(", "))
        return lines.joinToString("\n")
    }

    /** Shown for the second or two the ringing service checks with the server. */
    fun checking(context: Context): Notification =
        NotificationCompat.Builder(context, CHANNEL_SERVICE)
            .setSmallIcon(R.drawable.clockin_notification)
            .setContentTitle(context.getString(R.string.clockin_android_checking))
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setOngoing(true)
            .setSilent(true)
            .build()

    /**
     * Ring mode: the full-screen alarm (heads-up while the phone is in use).
     *
     * Never `setSilent`: Android's SystemUI refuses the full-screen intent of
     * a "silent" notification, and NotificationCompat's silent mode also puts
     * it in a group whose alerts are suppressed, which is refused too
     * (`FullScreenIntentDecisionProvider`). The channel itself has no sound;
     * the service plays the alarm. Updates (names changing) don't alert again.
     */
    fun ringing(context: Context, items: List<PlanItem>): Notification {
        val text = nameLines(context, items)
        return NotificationCompat.Builder(context, CHANNEL_ALARM)
            .setSmallIcon(R.drawable.clockin_notification)
            .setContentTitle(context.getString(R.string.clockin_alarm_title))
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setCategory(NotificationCompat.CATEGORY_ALARM)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setContentIntent(alarmScreenIntent(context, fullScreen = false))
            .setFullScreenIntent(alarmScreenIntent(context, fullScreen = true), true)
            .setDeleteIntent(repostIntent(context))
            .addAction(0, context.getString(R.string.clockin_alarm_stop), stopIntent(context))
            .build()
    }

    /** Ring mode, between rings: says when it rings again; Stop still works. */
    fun showSilent(context: Context, items: List<PlanItem>, reringTime: String) {
        val text = context.getString(R.string.clockin_alarm_silent, reringTime) + "\n" + nameLines(context, items)
        val notification = NotificationCompat.Builder(context, CHANNEL_SERVICE)
            .setSmallIcon(R.drawable.clockin_notification)
            .setContentTitle(context.getString(R.string.clockin_alarm_title))
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setCategory(NotificationCompat.CATEGORY_ALARM)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOngoing(true)
            .setSilent(true)
            .setContentIntent(alarmScreenIntent(context, fullScreen = false))
            .setDeleteIntent(repostIntent(context))
            .addAction(0, context.getString(R.string.clockin_alarm_stop), stopIntent(context))
            .build()
        notify(context, ID_SILENT, notification)
    }

    /**
     * Notification mode (SPEC §7.4): one notification, standard sound, opens
     * the app. While Do Not Disturb is on it makes no sound, whatever the DND
     * exceptions say (ring mode still rings through DND as an alarm).
     */
    fun showReminder(context: Context, items: List<PlanItem>, minute: Long) {
        val text = nameLines(context, items)
        val dnd = doNotDisturb(context)
        val notification = NotificationCompat.Builder(context, CHANNEL_REMINDERS)
            .setSmallIcon(R.drawable.clockin_notification)
            .setContentTitle(context.getString(R.string.clockin_alarm_title))
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setSilent(dnd)
            .setAutoCancel(true)
            .setContentIntent(openAppIntent(context))
            .build()
        notify(context, ID_NOTIFY_BASE + (minute % 100_000).toInt(), notification)
    }

    /** The plan reaches less than 3 days ahead: ask for the app to be opened. */
    fun showHorizonShort(context: Context) {
        val notification = NotificationCompat.Builder(context, CHANNEL_SERVICE)
            .setSmallIcon(R.drawable.clockin_notification)
            .setContentTitle(context.getString(R.string.clockin_android_horizonTitle))
            .setContentText(context.getString(R.string.clockin_android_horizonBody))
            .setStyle(NotificationCompat.BigTextStyle().bigText(context.getString(R.string.clockin_android_horizonBody)))
            .setSilent(true)
            .setAutoCancel(true)
            .setContentIntent(openAppIntent(context))
            .build()
        notify(context, ID_HORIZON, notification)
    }

    /**
     * After an update: a permission the alarms need is off (Android 14+
     * installers may switch full-screen alarms off on every update).
     */
    fun showSetupNeeded(context: Context) {
        val body = context.getString(R.string.clockin_android_setupBody)
        val notification = NotificationCompat.Builder(context, CHANNEL_REMINDERS)
            .setSmallIcon(R.drawable.clockin_notification)
            .setContentTitle(context.getString(R.string.clockin_android_setupTitle))
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .setContentIntent(openAppIntent(context))
            .build()
        notify(context, ID_SETUP, notification)
    }

    fun cancel(context: Context, id: Int) {
        manager(context).cancel(id)
    }

    /** Whether one of this app's notifications is on screen now. */
    fun isShowing(context: Context, id: Int): Boolean =
        try {
            manager(context).activeNotifications.any { it.id == id }
        } catch (e: Exception) {
            true
        }

    private fun notify(context: Context, id: Int, notification: Notification) {
        ensureChannels(context)
        try {
            manager(context).notify(id, notification)
        } catch (e: SecurityException) {
            // Notification permission missing: the checklist shows it.
        }
    }
}
