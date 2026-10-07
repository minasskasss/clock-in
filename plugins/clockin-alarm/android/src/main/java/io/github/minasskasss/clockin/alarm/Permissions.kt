package io.github.minasskasss.clockin.alarm

import android.Manifest
import android.app.Activity
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.webkit.WebView
import androidx.core.app.ActivityCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tauri.plugin.JSArray

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
        // Below Android 13 there is no permission to ask, but the user can
        // still switch the app's notifications off (all, or the alarm channel).
        val notifications = notificationsAllowed(context) && !Notifications.alarmChannelBlocked(context)
        val fullScreen = Build.VERSION.SDK_INT < 34 || nm.canUseFullScreenIntent()
        // App hibernation (Android 12+): an app not opened for months can't
        // run alarms or jobs. "Pause app activity if unused" must be off. On
        // Android 11 the same switch only resets runtime permissions, and the
        // app has none there (notifications became one in 13), so it can't
        // stop the alarms: the item isn't shown.
        val unusedExempt = Build.VERSION.SDK_INT < 31 || context.packageManager.isAutoRevokeWhitelisted
        val brand = Build.MANUFACTURER.lowercase()
        return mapOf(
            "notifications" to notifications,
            "exactAlarms" to AlarmScheduler.canScheduleExact(context),
            "fullScreen" to fullScreen,
            "battery" to pm.isIgnoringBatteryOptimizations(context.packageName),
            "unusedApps" to unusedExempt,
            // Redmi and POCO are Xiaomi phones with the same settings.
            "oem" to (OEM_BRANDS.firstOrNull { brand.contains(it) }
                ?.let { if (it == "redmi" || it == "poco") "xiaomi" else it } ?: ""),
            "oemDone" to Store.get(context).oemDone(),
            "sdk" to Build.VERSION.SDK_INT,
            "notApplicable" to JSArray().apply { notApplicable().forEach { put(it) } },
        )
    }

    /**
     * Checklist items with no setting on this Android version (always
     * allowed there), which the checklist doesn't show: exact alarms need no
     * permission before 12, full-screen alarms none before 14, and the
     * "unused app" switch can't affect the alarms before 12 (see above).
     */
    fun notApplicable(): List<String> = buildList {
        if (Build.VERSION.SDK_INT < 31) add("exactAlarms")
        if (Build.VERSION.SDK_INT < 34) add("fullScreen")
        if (Build.VERSION.SDK_INT < 31) add("unusedApps")
    }

    /** Everything the alarms need is granted (the maker's step is advice; same rule as Rust). */
    fun allGranted(context: Context): Boolean {
        val status = status(context)
        return listOf("notifications", "exactAlarms", "fullScreen", "battery", "unusedApps").all { status[it] == true }
    }

    /** The app may post notifications (Android 13+ asks; anyone can switch them off). */
    fun notificationsAllowed(context: Context): Boolean =
        NotificationManagerCompat.from(context).areNotificationsEnabled()

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
            // Xiaomi / Redmi / POCO (MIUI 12–14, HyperOS): their own screens,
            // else the app's page, where the same switches are on MIUI 13+.
            "xiaomiAutostart" -> listOf(
                component("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"),
                appDetails(context),
            )
            "xiaomiPermissions" -> listOf(
                Intent("miui.intent.action.APP_PERM_EDITOR")
                    .setClassName("com.miui.securitycenter", "com.miui.permcenter.permissions.PermissionsEditorActivity")
                    .putExtra("extra_pkgname", context.packageName),
                Intent("miui.intent.action.APP_PERM_EDITOR")
                    .setClassName("com.miui.securitycenter", "com.miui.permcenter.permissions.AppPermissionsEditorActivity")
                    .putExtra("extra_pkgname", context.packageName),
                Intent("miui.intent.action.APP_PERM_EDITOR").putExtra("extra_pkgname", context.packageName),
                appDetails(context),
            )
            "xiaomiBattery" -> listOf(
                component("com.miui.powerkeeper", "com.miui.powerkeeper.ui.HiddenAppsConfigActivity")
                    .putExtra("package_name", context.packageName)
                    .putExtra("package_label", context.applicationInfo.loadLabel(context.packageManager).toString()),
                appDetails(context),
            )
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
    private fun component(pkg: String, cls: String) = Intent().setComponent(ComponentName(pkg, cls))

    private fun oemIntents(): List<Intent> {
        val brand = Build.MANUFACTURER.lowercase()
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

    /** A system property, or "" (no secrets: OS version names only). */
    private fun systemProperty(name: String): String =
        try {
            val process = ProcessBuilder("getprop", name).redirectErrorStream(true).start()
            val value = process.inputStream.bufferedReader().use { it.readText() }.trim()
            process.waitFor()
            value
        } catch (e: Exception) {
            ""
        }

    /** The maker's own system and its version: HyperOS, MIUI or One UI, or "". */
    private fun makerOs(): String {
        val hyperOs = systemProperty("ro.mi.os.version.incremental")
        if (hyperOs.isNotEmpty()) return "HyperOS $hyperOs"
        val miui = systemProperty("ro.miui.ui.version.name")
        if (miui.isNotEmpty()) return "MIUI $miui (${Build.VERSION.INCREMENTAL})"
        val oneUi = systemProperty("ro.build.version.oneui").toIntOrNull()
        if (oneUi != null && oneUi > 0) return "One UI ${oneUi / 10000}.${oneUi / 100 % 100}"
        return ""
    }

    /**
     * The oldest WebView (Chromium) version the app's screens run on: the
     * web build's target in `vite.config.ts` (DECISIONS, Android 11).
     */
    const val MIN_WEBVIEW = 91

    /** The WebView the app's screens run in, or null if Android doesn't say. */
    fun webView(): PackageInfo? = WebView.getCurrentWebViewPackage()

    /** The WebView's major version, e.g. 120 for "120.0.6099.230", or 0. */
    fun webViewMajor(info: PackageInfo?): Int =
        info?.versionName?.substringBefore('.')?.toIntOrNull() ?: 0

    /** Too old for the app's screens: they may stay blank or not work. */
    fun webViewTooOld(): Boolean {
        val major = webViewMajor(webView())
        return major in 1 until MIN_WEBVIEW
    }

    /** The WebView's own page in the Play Store (Android System WebView, or Chrome on Android 7–9). */
    fun openWebViewStore(activity: Activity) {
        val pkg = webView()?.packageName ?: "com.google.android.webview"
        for (uri in listOf("market://details?id=$pkg", "https://play.google.com/store/apps/details?id=$pkg")) {
            try {
                activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
                return
            } catch (e: Exception) {
                // No Play Store: try the web page.
            }
        }
    }

    /** The phone and its systems, for the diagnostics view. */
    fun phone(): Map<String, Any> = mapOf(
        "manufacturer" to Build.MANUFACTURER.replaceFirstChar { it.uppercase() },
        "model" to Build.MODEL,
        "androidVersion" to Build.VERSION.RELEASE,
        "sdk" to Build.VERSION.SDK_INT,
        "makerOs" to makerOs(),
        "webViewVersion" to (webView()?.versionName ?: ""),
        "webViewPackage" to (webView()?.packageName ?: ""),
        "webViewOk" to !webViewTooOld(),
    )

    /** The phone's own name (e.g. "Galaxy S24 Ultra"), for the device list. */
    fun deviceName(context: Context): String {
        val named = Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)
        if (!named.isNullOrBlank()) return named.trim()
        val maker = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }
        return if (Build.MODEL.startsWith(maker, ignoreCase = true)) Build.MODEL else "$maker ${Build.MODEL}"
    }
}
