package io.github.minasskasss.clockin.alarm

import android.content.Context
import android.content.res.Configuration

/**
 * The app's theme choice for the native screens (alarm and reminder), with
 * the app's warm palettes (src/styles/tokens.css).
 */
internal object AppTheme {
    /**
     * Whether to draw dark. "Αυτόματο" uses the dark periods Rust computed
     * with the core's rule (dark from 21:00 until the business day ends);
     * without them yet, the phone's own setting.
     */
    fun dark(context: Context): Boolean {
        val systemDark =
            (context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        val store = Store.get(context)
        return when (store.theme()) {
            "light" -> false
            "dark" -> true
            "auto" -> {
                val windows = store.darkWindows()
                val now = System.currentTimeMillis()
                if (windows.isEmpty()) systemDark else windows.any { (start, end) -> now >= start && now < end }
            }
            else -> systemDark
        }
    }

    class Palette(dark: Boolean) {
        val bg: Int = android.graphics.Color.parseColor(if (dark) "#17120F" else "#F6EFE6")
        val text: Int = android.graphics.Color.parseColor(if (dark) "#F4EBE1" else "#2A211B")
        val muted: Int = android.graphics.Color.parseColor(if (dark) "#B9A999" else "#6E6056")
        val accent: Int = android.graphics.Color.parseColor(if (dark) "#F08A5D" else "#C2552F")
        val accentText: Int = android.graphics.Color.parseColor(if (dark) "#1C1410" else "#FFFAF4")
        /** `--go` / `--go-contrast`: the «Ναι, ήρθε» green. */
        val go: Int = android.graphics.Color.parseColor(if (dark) "#4FAE60" else "#2F7A3C")
        val goText: Int = android.graphics.Color.parseColor(if (dark) "#0E1F11" else "#F6FFF4")
        val warningBg: Int = android.graphics.Color.parseColor(if (dark) "#4A3610" else "#FDECC8")
        val warningText: Int = android.graphics.Color.parseColor(if (dark) "#FFE2A6" else "#5A3A05")
        val error: Int = android.graphics.Color.parseColor(if (dark) "#FF8A7E" else "#B3261E")
    }
}
