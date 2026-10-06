package io.github.minasskasss.clockin.alarm

import android.Manifest
import android.app.Activity
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

/**
 * The onboarding checklist (SPEC §8.2, ARCHITECTURE §10): what is granted,
 * and where to fix what isn't. Checked against the Android 14–17 docs
 * (DECISIONS, Phase 5).
 */
internal object Permissions {
    /** Phone makers whose own battery managers need an extra step (dontkillmyapp.com). */
    private val OEM_BRANDS = setOf("samsung", "xiaomi", "redmi", "poco", "huawei", "honor", "oppo", "realme", "oneplus", "vivo")

    fun status(context: Context): Map<String, Any> {
        val pm = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val notifications = (Build.VERSION.SDK_INT < 33 ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) &&
            !Notifications.alarmChannelBlocked(context)
        val fullScreen = Build.VERSION.SDK_INT < 34 || nm.canUseFullScreenIntent()
        // App hibernation (Android 11+): an app not opened for months loses
        // its alarms. "Pause app activity if unused" must be off.
        val unusedExempt = Build.VERSION.SDK_INT < 30 || context.packageManager.isAutoRevokeWhitelisted
        val brand = Build.MANUFACTURER.lowercase()
        return mapOf(
            "notifications" to notifications,
            "exactAlarms" to AlarmScheduler.canScheduleExact(context),
            "fullScreen" to fullScreen,
            "battery" to pm.isIgnoringBatteryOptimizations(context.packageName),
            "unusedApps" to unusedExempt,
            "oem" to (OEM_BRANDS.firstOrNull { brand.contains(it) } ?: ""),
            "oemDone" to Store.get(context).oemDone(),
        )
    }

    private fun packageUri(context: Context) = Uri.parse("package:" + context.packageName)

    private fun appDetails(context: Context) =
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, packageUri(context))

    private fun notificationSettings(context: Context) =
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)

    /** Opens the screen that fixes `kind`; the first tries of the notification permission ask directly. */
    fun open(activity: Activity, kind: String) {
        val context = activity.applicationContext
        val store = Store.get(context)
        val candidates: List<Intent> = when (kind) {
            "notifications" -> {
                val granted = Build.VERSION.SDK_INT < 33 ||
                    ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
                if (!granted && store.notificationRequests() < 2) {
                    store.countNotificationRequest()
                    ActivityCompat.requestPermissions(activity, arrayOf(Manifest.permission.POST_NOTIFICATIONS), 4201)
                    return
                }
                listOf(notificationSettings(context), appDetails(context))
            }
            "exactAlarms" -> if (Build.VERSION.SDK_INT >= 31) {
                listOf(Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, packageUri(context)), appDetails(context))
            } else {
                listOf(appDetails(context))
            }
            "fullScreen" -> if (Build.VERSION.SDK_INT >= 34) {
                listOf(Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, packageUri(context)), notificationSettings(context))
            } else {
                listOf(notificationSettings(context))
            }
            "battery" -> listOf(
                Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, packageUri(context)),
                Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
                appDetails(context),
            )
            "unusedApps" -> if (Build.VERSION.SDK_INT >= 30) {
                listOf(Intent(Intent.ACTION_AUTO_REVOKE_PERMISSIONS, packageUri(context)), appDetails(context))
            } else {
                listOf(appDetails(context))
            }
            "oem" -> oemIntents() + appDetails(context)
            else -> listOf(appDetails(context))
        }
        for (intent in candidates) {
            try {
                activity.startActivity(intent)
                return
            } catch (e: Exception) {
                // Not on this phone: try the next one.
            }
        }
    }

    /**
     * The makers' own background/autostart screens where a known one exists
     * (dontkillmyapp.com). Samsung's "never sleeping apps" list has no stable
     * link, so Samsung gets the app's own page (Battery → Unrestricted).
     */
    private fun oemIntents(): List<Intent> {
        val brand = Build.MANUFACTURER.lowercase()
        fun component(pkg: String, cls: String) = Intent().setComponent(ComponentName(pkg, cls))
        return when {
            brand.contains("xiaomi") || brand.contains("redmi") || brand.contains("poco") -> listOf(
                component("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"),
            )
            brand.contains("huawei") || brand.contains("honor") -> listOf(
                component("com.huawei.systemmanager", "com.huawei.systemmanager.startupmgr.ui.StartupNormalAppListActivity"),
                component("com.huawei.systemmanager", "com.huawei.systemmanager.optimize.process.ProtectActivity"),
            )
            brand.contains("oppo") || brand.contains("realme") -> listOf(
                component("com.coloros.safecenter", "com.coloros.safecenter.permission.startup.StartupAppListActivity"),
                component("com.oppo.safe", "com.oppo.safe.permission.startup.StartupAppListActivity"),
            )
            brand.contains("vivo") -> listOf(
                component("com.vivo.permissionmanager", "com.vivo.permissionmanager.activity.BgStartUpManagerActivity"),
            )
            else -> emptyList()
        }
    }

    /** The phone's own name (e.g. "Galaxy S24 Ultra"), for the device list. */
    fun deviceName(context: Context): String {
        val named = Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)
        if (!named.isNullOrBlank()) return named.trim()
        val maker = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }
        return if (Build.MODEL.startsWith(maker, ignoreCase = true)) Build.MODEL else "$maker ${Build.MODEL}"
    }
}
