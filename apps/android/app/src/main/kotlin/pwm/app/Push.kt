package pwm.app

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import kotlin.coroutines.resume
import kotlinx.coroutines.suspendCancellableCoroutine
import pwm.kit.ui.Notices
import pwm.kit.ui.PushService

// Push over FCM (D60): Firebase started from code with the build's settings,
// the messaging service, and the new-device notice. Nothing from Firebase
// but messaging, and a push never carries anything but its kind.

/** FCM, when the build has Firebase's settings; otherwise push is off. */
class FcmPush(context: Context) : PushService {
    private val app: FirebaseApp? = start(context)

    override val available: Boolean get() = app != null

    override suspend fun token(): String? {
        if (app == null) return null
        return suspendCancellableCoroutine { done ->
            FirebaseMessaging.getInstance().token.addOnCompleteListener { task ->
                done.resume(if (task.isSuccessful) task.result?.takeIf { it.isNotEmpty() } else null)
            }
        }
    }

    private companion object {
        fun start(context: Context): FirebaseApp? {
            val settings = listOf(BuildConfig.FIREBASE_PROJECT_ID, BuildConfig.FIREBASE_APP_ID, BuildConfig.FIREBASE_API_KEY, BuildConfig.FIREBASE_SENDER_ID)
            if (settings.any { it.isEmpty() }) return null
            FirebaseApp.getApps(context).firstOrNull()?.let { return it }
            val options = FirebaseOptions.Builder()
                .setProjectId(BuildConfig.FIREBASE_PROJECT_ID)
                .setApplicationId(BuildConfig.FIREBASE_APP_ID)
                .setApiKey(BuildConfig.FIREBASE_API_KEY)
                .setGcmSenderId(BuildConfig.FIREBASE_SENDER_ID)
                .build()
            // Firebase's own start-up provider is removed in the manifest, so
            // without these settings nothing of Firebase starts.
            return runCatching { FirebaseApp.initializeApp(context, options) }.getOrNull()
        }
    }
}

/** FCM's messages and new tokens go to the app's model. */
class PwmMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        (application as PwmApplication).model.pushTokenChanged(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        (application as PwmApplication).model.pushReceived(message.data["t"])
    }
}

/** The new-device notice (D60 §4), on its own channel; the words are the app's. */
class AndroidNotices(private val context: Context) : Notices {
    override fun allowed(): Boolean {
        val granted = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        return granted && NotificationManagerCompat.from(context).areNotificationsEnabled()
    }

    override fun showApprovalNotice() {
        if (!allowed()) return
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.notice_channel), NotificationManager.IMPORTANCE_HIGH),
        )
        val open = Intent(context, MainActivity::class.java)
            .setAction(OPEN_APPROVALS)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        val tap = PendingIntent.getActivity(context, 0, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notice = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notice)
            .setContentText(context.getString(R.string.approval_notice))
            .setStyle(NotificationCompat.BigTextStyle().bigText(context.getString(R.string.approval_notice)))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            // A request lasts 10 minutes (D46).
            .setTimeoutAfter(10 * 60_000L)
            .setAutoCancel(true)
            .setContentIntent(tap)
            .build()
        try {
            NotificationManagerCompat.from(context).notify(NOTICE_ID, notice)
        } catch (e: SecurityException) {
            // The permission went between the check and the notice.
        }
    }

    companion object {
        const val CHANNEL = "new_devices"
        const val NOTICE_ID = 1
        /** The notice's tap: Settings, then "Approve a new device". */
        const val OPEN_APPROVALS = "pwm.app.OPEN_APPROVALS"
    }
}
