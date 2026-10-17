package io.github.minasskasss.clockin.alarm

import android.app.Activity
import android.app.AlertDialog
import android.app.KeyguardManager
import android.content.Context
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.core.view.WindowCompat
import java.util.UUID
import java.util.concurrent.Executors

/**
 * The «Λείπει σήμανση» screen (SPEC §7.6), also over the lock screen: the
 * names still not marked, each with its own time and «Σήμανση», and a large
 * «Εντάξει». Native views like [AlarmActivity]. «Σήμανση» asks the same
 * question as the Today list, needs the phone unlocked, and marks through the
 * server directly (offline it says so and marks nothing). While the screen is
 * open it asks the server every 25 s, so names marked elsewhere disappear,
 * and it closes when nobody is left.
 */
class ReminderActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val listener: () -> Unit = { render() }
    private val recheckTick = Runnable { worker.execute { recheck() } }
    private lateinit var palette: AppTheme.Palette
    private lateinit var checkInTitle: TextView
    private lateinit var checkInRows: LinearLayout
    private lateinit var checkOutTitle: TextView
    private lateinit var checkOutRows: LinearLayout
    private lateinit var next: TextView
    private lateinit var message: TextView
    private var shownGroups: List<Long> = emptyList()

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
        val dark = AppTheme.dark(this)
        palette = AppTheme.Palette(dark)
        window.decorView.setBackgroundColor(palette.bg)
        if (Build.VERSION.SDK_INT < 35) {
            @Suppress("DEPRECATION")
            window.statusBarColor = palette.bg
        }
        WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = !dark
            isAppearanceLightNavigationBars = !dark
        }

        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24f), dp(40f), dp(24f), dp(32f))
        }
        column.addView(label(30f, palette.text, bold = true).apply { text = getString(R.string.clockin_reminder_title) })
        column.addView(label(17f, palette.muted).apply {
            text = getString(R.string.clockin_reminder_intro)
            setPadding(0, dp(8f), 0, 0)
        })
        checkInTitle = sectionTitle(R.string.clockin_reminder_checkIn)
        checkInRows = rows()
        checkOutTitle = sectionTitle(R.string.clockin_reminder_checkOut)
        checkOutRows = rows()
        listOf(checkInTitle, checkInRows, checkOutTitle, checkOutRows).forEach(column::addView)
        next = label(16f, palette.warningText).apply {
            setPadding(dp(14f), dp(10f), dp(14f), dp(10f))
            background = GradientDrawable().apply {
                cornerRadius = dp(10f).toFloat()
                setColor(palette.warningBg)
            }
        }
        column.addView(next, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply {
            topMargin = dp(24f)
        })
        message = label(16f, palette.error, bold = true).apply {
            visibility = View.GONE
            setPadding(0, dp(16f), 0, 0)
        }
        column.addView(message)

        val ok = Button(this).apply {
            text = getString(R.string.clockin_reminder_ok)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 26f)
            setTextColor(palette.accentText)
            isAllCaps = false
            typeface = Typeface.DEFAULT_BOLD
            background = GradientDrawable().apply {
                cornerRadius = dp(24f).toFloat()
                setColor(palette.accent)
            }
            minHeight = dp(84f)
            setOnClickListener {
                isEnabled = false
                Reminders.ok(this@ReminderActivity, shownGroups)
                finish()
            }
        }
        column.addView(ok, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply {
            topMargin = dp(28f)
        })
        column.addView(label(15f, palette.muted).apply {
            text = getString(R.string.clockin_reminder_note)
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(0, dp(16f), 0, 0)
        })

        val scroll = ScrollView(this).apply {
            isFillViewport = true
            addView(column)
        }
        scroll.setOnApplyWindowInsetsListener { view, insets ->
            @Suppress("DEPRECATION")
            view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            insets
        }
        setContentView(scroll)
        render()
    }

    private fun label(sizeSp: Float, color: Int, bold: Boolean = false) = TextView(this).apply {
        setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
        setTextColor(color)
        if (bold) typeface = Typeface.DEFAULT_BOLD
    }

    private fun sectionTitle(text: Int) = label(15f, palette.muted, bold = true).apply {
        this.text = getString(text)
        isAllCaps = true
        setPadding(0, dp(22f), 0, dp(4f))
    }

    private fun rows() = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

    override fun onStart() {
        super.onStart()
        Ring.addListener(listener)
        render()
        main.postDelayed(recheckTick, AlarmScheduler.RECHECK_MS)
    }

    override fun onStop() {
        main.removeCallbacks(recheckTick)
        Ring.removeListener(listener)
        super.onStop()
    }

    override fun onDestroy() {
        worker.shutdown()
        super.onDestroy()
    }

    private fun render() {
        val (groups, items) = Reminders.showing(this)
        if (items.isEmpty()) {
            finish()
            return
        }
        shownGroups = groups
        fill(checkInTitle, checkInRows, items.filter { it.kind == "in" })
        fill(checkOutTitle, checkOutRows, items.filter { it.kind == "out" })
        val at = Reminders.nextShown(this)
        next.text = if (at != null) getString(R.string.clockin_reminder_next, Ring.hhmm(at)) else getString(R.string.clockin_reminder_last)
    }

    private fun fill(title: TextView, rows: LinearLayout, items: List<PlanItem>) {
        rows.removeAllViews()
        val visibility = if (items.isEmpty()) View.GONE else View.VISIBLE
        title.visibility = visibility
        rows.visibility = visibility
        for (item in items) {
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, dp(8f), 0, dp(8f))
            }
            row.addView(label(24f, palette.accent, bold = true).apply {
                text = Ring.hhmm(item.firesAtMs)
                setPadding(0, 0, dp(12f), 0)
            })
            row.addView(
                label(24f, palette.text, bold = true).apply { text = item.name },
                LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f),
            )
            val checkIn = item.kind == "in"
            row.addView(Button(this).apply {
                text = getString(R.string.clockin_reminder_mark)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 18f)
                isAllCaps = false
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(if (checkIn) palette.goText else palette.accentText)
                background = GradientDrawable().apply {
                    cornerRadius = dp(14f).toFloat()
                    setColor(if (checkIn) palette.go else palette.accent)
                }
                minHeight = dp(52f)
                setPadding(dp(18f), 0, dp(18f), 0)
                setOnClickListener { confirm(item) }
            })
            rows.addView(row)
        }
    }

    /** The same question as on the Today list (SPEC §6), then unlock, then mark. */
    private fun confirm(item: PlanItem) {
        message.visibility = View.GONE
        val checkIn = item.kind == "in"
        AlertDialog.Builder(this)
            .setTitle(if (checkIn) R.string.clockin_confirm_inTitle else R.string.clockin_confirm_outTitle)
            .setMessage(
                getString(if (checkIn) R.string.clockin_confirm_inBody else R.string.clockin_confirm_outBody, item.name) +
                    "\n\n" + getString(R.string.clockin_confirm_noUndo),
            )
            .setNegativeButton(R.string.clockin_reminder_cancel, null)
            .setPositiveButton(if (checkIn) R.string.clockin_confirm_inYes else R.string.clockin_confirm_outYes) { _, _ -> unlockThenMark(item) }
            .show()
    }

    private fun unlockThenMark(item: PlanItem) {
        val keyguard = getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager
        if (!keyguard.isKeyguardLocked) {
            mark(item)
            return
        }
        keyguard.requestDismissKeyguard(this, object : KeyguardManager.KeyguardDismissCallback() {
            override fun onDismissSucceeded() = mark(item)
            override fun onDismissCancelled() = show(getString(R.string.clockin_reminder_unlockToMark))
            override fun onDismissError() = show(getString(R.string.clockin_reminder_unlockToMark))
        })
    }

    private fun mark(item: PlanItem) {
        worker.execute {
            val store = Store.get(this)
            val api = store.serverApi()
            val secret = Secrets.get(this, Secrets.DEVICE_SECRET)
            val answer = if (api != null && secret != null) api.mark(secret, UUID.randomUUID().toString(), item) else ServerApi.Answer.Unreachable
            main.post {
                if (answer is ServerApi.Answer.Ok) {
                    Reminders.drop(this, setOf(item.key))
                    show(getString(R.string.clockin_reminder_marked), error = false)
                } else {
                    show(getString(R.string.clockin_reminder_offline))
                }
            }
        }
    }

    private fun show(text: String, error: Boolean = true) {
        message.text = text
        message.setTextColor(if (error) palette.error else palette.go)
        message.visibility = View.VISIBLE
    }

    /** Worker thread: drops names marked, moved or removed elsewhere. A failed check keeps them. */
    private fun recheck() {
        val items = Reminders.showing(this).second
        val due = Checker.stillDue(this, items).map { it.key }.toSet()
        val gone = items.map { it.key }.filter { it !in due }.toSet()
        if (gone.isNotEmpty()) Reminders.drop(this, gone)
        main.post {
            if (!isFinishing) {
                main.removeCallbacks(recheckTick)
                main.postDelayed(recheckTick, AlarmScheduler.RECHECK_MS)
            }
        }
    }

    /** Back only hides the screen; the reminder stays until «Εντάξει». */
    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        moveTaskToBack(true)
    }
}
