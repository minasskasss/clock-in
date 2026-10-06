package io.github.minasskasss.clockin.alarm

import android.app.Activity
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * The full-screen alarm (SPEC §7.3): the names and one large «Σταμάτημα»,
 * also over the lock screen. Native views, so it opens fast and works
 * before the phone's first unlock. Colours follow the phone's dark mode,
 * with the app's warm palettes (src/styles/tokens.css).
 */
class AlarmActivity : Activity() {
    private lateinit var time: TextView
    private lateinit var checkInTitle: TextView
    private lateinit var checkInNames: TextView
    private lateinit var checkOutTitle: TextView
    private lateinit var checkOutNames: TextView
    private lateinit var silent: TextView
    private val listener: () -> Unit = { render() }

    private val dark: Boolean
        get() = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES

    private fun dp(value: Float): Int =
        TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value, resources.displayMetrics).toInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 27) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        val bg = Color.parseColor(if (dark) "#17120F" else "#F6EFE6")
        val text = Color.parseColor(if (dark) "#F4EBE1" else "#2A211B")
        val muted = Color.parseColor(if (dark) "#B9A999" else "#6E6056")
        val accent = Color.parseColor(if (dark) "#F08A5D" else "#C2552F")
        val accentText = Color.parseColor(if (dark) "#1C1410" else "#FFFAF4")
        window.decorView.setBackgroundColor(bg)
        window.statusBarColorCompat(bg)

        fun label(sizeSp: Float, color: Int, bold: Boolean = false) = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
            setTextColor(color)
            gravity = Gravity.CENTER_HORIZONTAL
            if (bold) typeface = Typeface.DEFAULT_BOLD
        }

        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(dp(24f), dp(48f), dp(24f), dp(32f))
        }
        column.addView(label(20f, muted).apply { text = getString(R.string.clockin_alarm_title) })
        time = label(64f, text, bold = true)
        column.addView(time)
        checkInTitle = label(18f, muted).apply { text = getString(R.string.clockin_alarm_checkIn); setPadding(0, dp(20f), 0, 0) }
        checkInNames = label(30f, text, bold = true)
        checkOutTitle = label(18f, muted).apply { text = getString(R.string.clockin_alarm_checkOut); setPadding(0, dp(20f), 0, 0) }
        checkOutNames = label(30f, text, bold = true)
        silent = label(17f, muted).apply { setPadding(0, dp(20f), 0, 0) }
        listOf(checkInTitle, checkInNames, checkOutTitle, checkOutNames, silent).forEach(column::addView)

        val stop = Button(this).apply {
            text = getString(R.string.clockin_alarm_stop)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 28f)
            setTextColor(accentText)
            isAllCaps = false
            typeface = Typeface.DEFAULT_BOLD
            background = GradientDrawable().apply {
                cornerRadius = dp(24f).toFloat()
                setColor(accent)
            }
            minHeight = dp(96f)
            setOnClickListener {
                Ring.stop(this@AlarmActivity)
                finish()
            }
        }
        column.addView(stop, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply {
            topMargin = dp(36f)
        })
        column.addView(label(15f, muted).apply {
            text = getString(R.string.clockin_alarm_note)
            setPadding(0, dp(20f), 0, 0)
        })

        val scroll = ScrollView(this).apply {
            isFillViewport = true
            addView(column)
        }
        // Edge to edge (Android 15+): keep clear of the status and navigation bars.
        scroll.setOnApplyWindowInsetsListener { view, insets ->
            @Suppress("DEPRECATION")
            view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            insets
        }
        setContentView(scroll)
        render()
    }

    private fun android.view.Window.statusBarColorCompat(color: Int) {
        if (Build.VERSION.SDK_INT < 35) {
            @Suppress("DEPRECATION")
            statusBarColor = color
        }
    }

    override fun onStart() {
        super.onStart()
        Ring.addListener(listener)
        render()
    }

    override fun onStop() {
        Ring.removeListener(listener)
        super.onStop()
    }

    private fun render() {
        val active = Store.get(this).active()
        if (active == null) {
            finish()
            return
        }
        time.text = Ring.hhmm(active.items.minOf { it.firesAtMs })
        val checkIn = active.items.filter { it.kind == "in" }.map { it.name }.distinct()
        val checkOut = active.items.filter { it.kind == "out" }.map { it.name }.distinct()
        checkInNames.text = checkIn.joinToString("\n")
        checkOutNames.text = checkOut.joinToString("\n")
        val inVisibility = if (checkIn.isEmpty()) View.GONE else View.VISIBLE
        val outVisibility = if (checkOut.isEmpty()) View.GONE else View.VISIBLE
        checkInTitle.visibility = inVisibility
        checkInNames.visibility = inVisibility
        checkOutTitle.visibility = outVisibility
        checkOutNames.visibility = outVisibility
        val rering = active.reringAtMs
        silent.visibility = if (rering == null) View.GONE else View.VISIBLE
        if (rering != null) silent.text = getString(R.string.clockin_alarm_silent, Ring.hhmm(rering))
    }

    /** Back only hides the screen; the alarm goes on until «Σταμάτημα». */
    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        moveTaskToBack(true)
    }
}
