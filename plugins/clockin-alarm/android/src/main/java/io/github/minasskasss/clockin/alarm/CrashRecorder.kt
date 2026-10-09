package io.github.minasskasss.clockin.alarm

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.Build

/**
 * Keeps the last uncaught Kotlin/Java exception (time, app version, error)
 * for «Διαγνωστικά» (PLAN Phase 6). Rust keeps its own last panic and
 * Android its own record of native crashes and ANRs; the newest is shown.
 *
 * Android creates content providers when the process starts, before any
 * activity, receiver or service, so this one installs the handler for the
 * app, the alarm receivers and the ringing service alike. It serves no data.
 */
class CrashRecorder : ContentProvider() {
    companion object {
        /** Messages are cut to this many characters. */
        private const val MAX_MESSAGE = 160

        fun install(context: Context) {
            val previous = Thread.getDefaultUncaughtExceptionHandler()
            if (previous is Recorder) return
            Thread.setDefaultUncaughtExceptionHandler(Recorder(context.applicationContext, previous))
        }

        /** "IllegalStateException: <message> (RingService.kt:123)", on one line, no stack trace. */
        fun describe(e: Throwable): String {
            val frame = e.stackTrace.firstOrNull { it.className.startsWith("io.github.minasskasss.") }
                ?: e.stackTrace.firstOrNull()
            val where = frame?.let { " (${it.fileName}:${it.lineNumber})" } ?: ""
            return "${e.javaClass.simpleName}: ${oneLine(e.message)}$where"
        }

        private fun oneLine(text: String?): String = (text ?: "")
            .map { if (it.isISOControl()) ' ' else it }
            .joinToString("")
            .let { if (it.length > MAX_MESSAGE) it.take(MAX_MESSAGE) + "…" else it }

        /**
         * The last native crash or ANR of this installed version, from
         * Android's own record of how the app's processes ended (Android 11+).
         * The handler above never sees these: for example, a crashed WebView
         * renderer takes the app down with a native signal.
         */
        fun lastFromSystem(context: Context): Triple<Long, String, String>? {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
            return try {
                val info = context.packageManager.getPackageInfo(context.packageName, 0)
                val am = context.getSystemService(ActivityManager::class.java) ?: return null
                val exit = am.getHistoricalProcessExitReasons(context.packageName, 0, 0)
                    .filter { it.timestamp >= info.lastUpdateTime }
                    .filter { it.reason == ApplicationExitInfo.REASON_CRASH_NATIVE || it.reason == ApplicationExitInfo.REASON_ANR }
                    .maxByOrNull { it.timestamp } ?: return null
                val error = when (exit.reason) {
                    ApplicationExitInfo.REASON_ANR -> "ANR: ${oneLine(exit.description)}"
                    else -> "Native crash, signal ${exit.status}"
                }
                Triple(exit.timestamp, info.versionName ?: "", error)
            } catch (_: Exception) {
                null
            }
        }
    }

    private class Recorder(
        private val context: Context,
        private val previous: Thread.UncaughtExceptionHandler?,
    ) : Thread.UncaughtExceptionHandler {
        override fun uncaughtException(thread: Thread, e: Throwable) {
            try {
                val version = try {
                    context.packageManager.getPackageInfo(context.packageName, 0).versionName ?: ""
                } catch (_: Exception) {
                    ""
                }
                Store.get(context).recordCrash(System.currentTimeMillis(), version, describe(e))
            } catch (_: Throwable) {
                // Never get in the way of the crash itself.
            }
            previous?.uncaughtException(thread, e)
        }
    }

    override fun onCreate(): Boolean {
        context?.let(::install)
        return true
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null

    override fun getType(uri: Uri): String? = null

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0
}
