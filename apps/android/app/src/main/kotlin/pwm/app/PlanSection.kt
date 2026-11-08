package pwm.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import java.text.DateFormat
import java.util.Date
import pwm.kit.BillingStatus
import pwm.kit.design.Tokens
import pwm.kit.ui.PlanModel

/**
 * The Plan section of Settings (D6 §2-4): the plan and where it's managed.
 * On a free plan, where the server sells through Google Play, one button
 * buys Premium there; nothing else here asks to buy. A Play subscription is
 * canceled on Play's own page, which "Manage subscription" opens. Within
 * 30 days of a charge, "Request a refund" ends a Play or web plan now and
 * pays back the unused days (D72 §1); only Apple refunds an App Store plan.
 */
@Composable
internal fun PlanSection(model: PlanModel) {
    val state by model.state.collectAsState()
    val context = LocalContext.current
    LaunchedEffect(model) { model.load() }
    val status = state.status
    if (status == null) {
        if (state.error == null) {
            Caption(stringResource(R.string.plan_loading))
        } else {
            ErrorLine(state.error)
            Secondary(stringResource(R.string.plan_retry), tag = "planRetry", onClick = model::load)
        }
        return
    }
    Text(
        planName(status.plan), style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text),
        modifier = Modifier.testTag("planName"),
    )
    if (state.includedWithServer) Caption(stringResource(R.string.plan_included))
    val subscription = status.subscription
    if (subscription != null) {
        val day = DateFormat.getDateInstance(DateFormat.LONG).format(Date(subscription.untilMs))
        Caption(stringResource(if (subscription.canceling) R.string.plan_ends else R.string.plan_renews, day))
        if (subscription.status == "past_due") {
            Text(
                stringResource(R.string.plan_past_due), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.warning),
            )
        }
        when (subscription.provider) {
            BillingStatus.PLAY -> {
                Caption(stringResource(R.string.plan_managed_play))
                Secondary(stringResource(R.string.plan_manage), tag = "planManage") { open(context, model.manageUrl) }
            }
            BillingStatus.APP_STORE -> Caption(stringResource(R.string.plan_managed_app_store), tag = "planManagedElsewhere")
            BillingStatus.STRIPE -> Caption(stringResource(R.string.plan_managed_web), tag = "planManagedElsewhere")
        }
        val refundableUntil = subscription.refundableUntilMs
        if (refundableUntil != null && state.refundOffered) {
            val until = DateFormat.getDateInstance(DateFormat.LONG).format(Date(refundableUntil))
            Secondary(stringResource(R.string.plan_refund), tag = "planRefund", enabled = state.canRefund, onClick = model::refund)
            Caption(stringResource(R.string.plan_refund_hint, until))
        }
        if (state.refundFromApple) Caption(stringResource(R.string.plan_refund_apple), tag = "planRefundApple")
    }
    if (state.refunded && subscription == null) {
        Text(
            stringResource(R.string.plan_refunded), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text),
            modifier = Modifier.testTag("planRefunded"),
        )
    }
    if (state.alsoPaying.isNotEmpty()) {
        val where = state.alsoPaying.map {
            stringResource(
                when (it) {
                    BillingStatus.PLAY -> R.string.plan_also_play
                    BillingStatus.APP_STORE -> R.string.plan_also_app_store
                    else -> R.string.plan_also_web
                },
            )
        }
        Text(
            stringResource(R.string.plan_also, where.joinToString(" and ")),
            style = Tokens.Text.caption.compose(), color = color(Tokens.Color.warning), modifier = Modifier.testTag("planAlso"),
        )
        if (BillingStatus.PLAY in state.alsoPaying && subscription?.provider != BillingStatus.PLAY) {
            Secondary(stringResource(R.string.plan_open_play), tag = "planOpenPlay") { open(context, model.playSubscriptionsUrl) }
        }
    }
    if (state.pending) {
        Text(
            stringResource(R.string.plan_pending), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text),
            modifier = Modifier.testTag("planPending"),
        )
    }
    if (state.canBuy || (state.busy && status.plan == BillingStatus.FREE)) {
        Primary(stringResource(R.string.plan_get_premium), enabled = state.canBuy, tag = "getPremium", onClick = model::buy)
        Caption(stringResource(R.string.plan_get_premium_hint))
    }
    ErrorLine(state.error)
}

@Composable
private fun planName(plan: String): String = when (plan) {
    BillingStatus.FREE -> stringResource(R.string.plan_free)
    "premium" -> stringResource(R.string.plan_premium)
    "family" -> stringResource(R.string.plan_family)
    "teams" -> stringResource(R.string.plan_teams)
    // A plan a newer server names: its own word.
    else -> plan
}

@Composable
private fun Caption(text: String, tag: String? = null) {
    Text(
        text, style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted),
        modifier = if (tag != null) Modifier.testTag(tag) else Modifier,
    )
}

/** Play's own page, in the Play Store app or the browser. */
private fun open(context: Context, url: String) {
    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
}
