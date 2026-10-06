package io.github.minasskasss.clockin.alarm

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

/**
 * Background refresh (ARCHITECTURE §10): every 15 minutes with a network,
 * asks the server whether the alarm plan changed and, if so, downloads it
 * and reschedules. Also extends the 48-hour scheduling window from the
 * stored plan, and asks for the app to be opened when the plan reaches
 * less than 3 days ahead.
 */
class PlanWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    companion object {
        private const val PERIODIC = "clockin-plan-refresh"
        private const val ONCE = "clockin-plan-refresh-now"
        /** The banner's threshold (SPEC §6), from `clockin-core::HORIZON_WARNING_BELOW`. */
        private const val HORIZON_WARNING_MS = 3L * 24 * 60 * 60 * 1000

        private fun constraints() = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()

        fun ensurePeriodic(context: Context) {
            try {
                WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                    PERIODIC,
                    ExistingPeriodicWorkPolicy.KEEP,
                    PeriodicWorkRequestBuilder<PlanWorker>(15, TimeUnit.MINUTES).setConstraints(constraints()).build(),
                )
            } catch (e: Exception) {
                // Before the first unlock WorkManager isn't available; boot retries later.
            }
        }

        fun refreshSoon(context: Context) {
            try {
                WorkManager.getInstance(context).enqueueUniqueWork(
                    ONCE,
                    ExistingWorkPolicy.REPLACE,
                    OneTimeWorkRequestBuilder<PlanWorker>().setConstraints(constraints()).build(),
                )
            } catch (e: Exception) {
            }
        }
    }

    override fun doWork(): Result {
        val context = applicationContext
        try {
            refresh(context)
        } finally {
            AlarmScheduler.reschedule(context)
        }
        return Result.success()
    }

    private fun refresh(context: Context) {
        val store = Store.get(context)
        val api = store.serverApi() ?: return
        val secret = Secrets.get(context, Secrets.DEVICE_SECRET) ?: return
        val version = when (val answer = api.getVersion(secret)) {
            is ServerApi.Answer.Ok -> answer.body
            is ServerApi.Answer.Rejected -> {
                if (answer.unpaired) synchronized(Store.LOCK) { store.clearPlan() }
                return
            }
            ServerApi.Answer.Unreachable -> return
        }
        val planVersion = version.optLong("plan_config_version", 0)
        val horizon = version.optString("plan_horizon_end", "").takeIf { it.isNotEmpty() && it != "null" }
            ?.let { PlanItem.parseInstantMs(it) }

        // Never replace a newer plan (Rust's, from a newer configuration)
        // with an older one; take the server's when it is newer or reaches
        // further for the same configuration.
        val stored = store.plan()
        val newer = planVersion > stored.configVersion ||
            (planVersion == stored.configVersion && horizon != null && horizon > (stored.horizonEndMs ?: 0))
        if (newer) {
            val plan = (api.getPlan(secret) as? ServerApi.Answer.Ok)?.body ?: return
            val list = plan.optJSONArray("items") ?: return
            val items = (0 until list.length()).mapNotNull { PlanItem.fromJson(list.getJSONObject(it)) }
            val planHorizon = plan.optString("horizon_end", "").let { PlanItem.parseInstantMs(it) }
            synchronized(Store.LOCK) {
                store.setPlan(Plan(plan.optLong("plan_config_version", planVersion), planHorizon, items))
            }
            Ring.onPlanChanged(context)
        }

        val now = System.currentTimeMillis()
        if (horizon == null || horizon < now + HORIZON_WARNING_MS) {
            Notifications.showHorizonShort(context)
        } else {
            Notifications.cancel(context, Notifications.ID_HORIZON)
        }
    }
}
