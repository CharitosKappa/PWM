package pwm.app

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import pwm.kit.ui.BackgroundScheduler

/**
 * The periodic background sync (D57 §2): the core's locked sync with the
 * stored tokens, so no master password is needed. Android may run it later
 * than asked, in Doze or to save battery.
 */
class SyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        // Offline or a server error: the next period tries again, so every run succeeds.
        (applicationContext as PwmApplication).model.backgroundSync()
        return Result.success()
    }
}

/** Every 15 minutes, the shortest Android allows, with a network connection; while someone is signed in. */
class WorkManagerScheduler(private val context: Context) : BackgroundScheduler {
    override fun schedule() {
        val request = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(NAME, ExistingPeriodicWorkPolicy.KEEP, request)
    }

    override fun cancel() {
        WorkManager.getInstance(context).cancelUniqueWork(NAME)
    }

    companion object {
        const val NAME = "pwm.sync"
    }
}
