package pwm.kit.ui

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Account
import pwm.kit.BillingStatus
import pwm.kit.Messages
import pwm.kit.PlayProducts
import pwm.kit.PwmError
import pwm.kit.Redacted

// The plan in Settings (D6 §2-4): what the account has, where it's managed,
// on a server that sells through Google Play, buying Premium there, and
// within 30 days of a charge, a refund (D72 §1).

/** A subscription Play reports for this phone's Google account. Never printed. */
class PlayPurchase(
    val productIds: List<String>,
    /** Handed to the server, which reads the purchase from Google. */
    val token: Redacted<String>,
    val state: State,
    /** The `obfuscatedAccountId` it was bought with: our account's billing token. */
    val accountToken: Redacted<String>?,
    val acknowledged: Boolean,
) {
    enum class State { PURCHASED, PENDING, UNSPECIFIED }

    override fun toString(): String = "PlayPurchase($state)"
}

/** What Play answered. */
sealed class PlayResult {
    class Purchases(val purchases: List<PlayPurchase>) : PlayResult()

    /** The user closed Play's sheet. */
    data object Canceled : PlayResult()

    /** This Google account has it already. */
    data object AlreadyOwned : PlayResult()

    /** No Play Store, not signed in to it, or the product isn't set up. */
    data object Unavailable : PlayResult()

    data object Failed : PlayResult()
}

/**
 * Google Play Billing (D6 §2, §10): the Play Billing Library in the app, a
 * fake in tests. It never acknowledges a purchase (the server does) and
 * never logs one.
 */
interface PlayBilling {
    /** The app's package, for Play's subscription page. */
    val packageName: String

    /** Purchases Play reports outside [buy], such as a pending payment that went through. */
    val results: Flow<PlayResult>

    /** Shows Play's sheet for [productId] with [accountToken] as its `obfuscatedAccountId`, and waits for its answer. */
    suspend fun buy(productId: String, accountToken: Redacted<String>): PlayResult

    /** The subscriptions Play holds for this Google account (`queryPurchasesAsync`); empty without Play. */
    suspend fun owned(): List<PlayPurchase>
}

object NoPlayBilling : PlayBilling {
    override val packageName: String = ""
    override val results: Flow<PlayResult> = emptyFlow()

    override suspend fun buy(productId: String, accountToken: Redacted<String>): PlayResult = PlayResult.Unavailable

    override suspend fun owned(): List<PlayPurchase> = emptyList()
}

/**
 * The Plan section of Settings. The server says what the plan is; Play only
 * sells it. A purchase Play finished goes to the server at once, and one
 * that finished while the app was closed or locked goes after unlocking.
 * Nothing here prompts: buying is one button in Settings, and so is a
 * refund while one applies.
 */
class PlanModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    private val play: PlayBilling,
) {
    data class State(
        val status: BillingStatus? = null,
        val loading: Boolean = false,
        /** A Play payment is waiting (a cash or bank payment, say). */
        val pending: Boolean = false,
        val busy: Boolean = false,
        /** A refund went through: the plan ended and the money is on its way. */
        val refunded: Boolean = false,
        val error: String? = null,
    ) {
        val plan: String? get() = status?.plan

        /** A server that sells nothing (self-hosted): every account has Premium. */
        val includedWithServer: Boolean get() = status?.sells?.isEmpty() == true

        /** Where the plan is managed: `play`, `app_store` or `stripe`; null without a subscription. */
        val managedBy: String? get() = status?.subscription?.provider

        /** Buying shows only on a free plan, on a server selling through Play, and not while a payment waits. */
        val canBuy: Boolean
            get() = status?.let { it.plan == BillingStatus.FREE && BillingStatus.PLAY in it.sells && it.token != null } == true &&
                !pending && !busy

        /**
         * "Request a refund" shows while the server says one applies (D72 §1),
         * for a subscription bought through Stripe or Play. Only Apple refunds
         * an App Store purchase.
         */
        val refundOffered: Boolean
            get() = status?.subscription?.let { it.refundableUntilMs != null && it.provider in REFUNDED_HERE } == true

        /** The refund button is enabled: offered, and nothing else under way. */
        val canRefund: Boolean get() = refundOffered && !busy

        /** An App Store plan: refunds are asked of Apple. */
        val refundFromApple: Boolean get() = status?.subscription?.provider == BillingStatus.APP_STORE

        /** Second subscriptions, bought by mistake: where to cancel each. */
        val alsoPaying: List<String> get() = status?.subscription?.also.orEmpty()
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    private var listening: Job? = null

    /** Play's subscription page for Premium, where it's canceled (D6 §5 §4). */
    val manageUrl: String get() = "$PLAY_SUBSCRIPTIONS?sku=${PlayProducts.PREMIUM}&package=${play.packageName}"

    /** Play's list of subscriptions, for a second one to cancel. */
    val playSubscriptionsUrl: String get() = PLAY_SUBSCRIPTIONS

    /**
     * After unlocking: listens to Play, and hands over a purchase that
     * finished while the app was closed. The server is asked only when Play
     * holds one of ours that it hasn't acknowledged.
     */
    fun start() {
        if (listening != null) return
        listening = scope.launch { play.results.collect { answered(it) } }
        scope.launch {
            val owned = try {
                play.owned()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return@launch
            }
            handOver(owned, quiet = true)
        }
    }

    /** Locking or signing out: Play's later answers wait for the next unlock. */
    fun stop() {
        listening?.cancel()
        listening = null
    }

    /** Settings shows the plan: read from the server each time. */
    fun load() {
        scope.launch {
            if (refresh() == null) return@launch
            if (BillingStatus.PLAY in mutable.value.status?.sells.orEmpty()) {
                val owned = try {
                    play.owned()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    return@launch
                }
                handOver(owned, quiet = true)
            }
        }
    }

    /** "Get Premium": Play's sheet, then the purchase to the server. */
    fun buy() {
        val s = mutable.value
        if (!s.canBuy) return
        val token = s.status?.token ?: return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                answered(play.buy(PlayProducts.PREMIUM, token))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = planMessage(PwmError("play_failed"))) }
            } finally {
                mutable.update { it.copy(busy = false) }
            }
        }
    }

    /**
     * "Request a refund": the server pays back the unused days and ends the
     * plan now (D72 §1). The account keeps all its data on Free.
     */
    fun refund() {
        val s = mutable.value
        if (!s.canRefund) return
        val provider = s.status?.subscription?.provider ?: return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                val answer = account.refund(provider)
                mutable.update { it.copy(status = answer.copy(token = answer.token ?: it.status?.token), refunded = true) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = planMessage(e)) }
            } finally {
                mutable.update { it.copy(busy = false) }
            }
        }
    }

    private suspend fun answered(result: PlayResult) {
        when (result) {
            is PlayResult.Purchases -> handOver(result.purchases, quiet = false)
            PlayResult.Canceled -> Unit
            PlayResult.AlreadyOwned -> handOver(runCatching { play.owned() }.getOrDefault(emptyList()), quiet = false)
            PlayResult.Unavailable -> mutable.update { it.copy(error = planMessage(PwmError("play_unavailable"))) }
            PlayResult.Failed -> mutable.update { it.copy(error = planMessage(PwmError("play_failed"))) }
        }
    }

    /**
     * Sends our Premium purchases to the server: those bought with this
     * account's token that the server hasn't acknowledged, or any while the
     * server shows no Play subscription. Another account's are left alone.
     */
    private suspend fun handOver(purchases: List<PlayPurchase>, quiet: Boolean) {
        val ours = purchases.filter { PlayProducts.PREMIUM in it.productIds }
        val waiting = ours.filter { it.state == PlayPurchase.State.PENDING || (it.state == PlayPurchase.State.PURCHASED && !it.acknowledged) }
        // Nothing for the server: no call, unless the plan is already showing.
        val status = mutable.value.status ?: if (waiting.isEmpty()) return else refresh() ?: return
        val token = status.token?.value ?: return
        val mine = ours.filter { it.accountToken?.value == token }
        val playKnown = status.subscription?.provider == BillingStatus.PLAY
        val send = mine.filter { it.state == PlayPurchase.State.PURCHASED && (!it.acknowledged || !playKnown) }
        var pending = mine.any { it.state == PlayPurchase.State.PENDING }
        var error: String? = null
        for (purchase in send) {
            try {
                val answer = account.playPurchase(PlayProducts.PREMIUM, purchase.token)
                mutable.update { it.copy(status = answer.copy(token = answer.token ?: it.status?.token)) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                val code = PwmError.from(e).code
                if (code == "purchase_pending") pending = true else error = planMessage(e)
            }
        }
        if (quiet && send.isEmpty() && !pending) return
        mutable.update { it.copy(pending = pending && it.status?.plan == BillingStatus.FREE, error = error ?: it.error) }
    }

    /** The plan from the server; null on failure, which shows. */
    private suspend fun refresh(): BillingStatus? {
        mutable.update { it.copy(loading = true, refunded = false, error = null) }
        return try {
            account.billingStatus().also { status -> mutable.update { it.copy(status = status, loading = false) } }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(loading = false, error = planMessage(e)) }
            null
        }
    }

    private companion object {
        const val PLAY_SUBSCRIPTIONS = "https://play.google.com/store/account/subscriptions"

        /** Where the server refunds (D72 §1). */
        val REFUNDED_HERE = setOf(BillingStatus.STRIPE, BillingStatus.PLAY)

        /**
         * Google Play's own codes. The server's billing codes (D6 §7) are in
         * design/messages.json, shared with the Apple apps; a pending
         * payment gets Play's wording here.
         */
        val MESSAGES = mapOf(
            "purchase_pending" to "Waiting for Google Play to confirm the payment.",
            "play_unavailable" to "Google Play isn't available on this phone.",
            "play_failed" to "Google Play couldn't finish the purchase. Try again.",
        )

        fun planMessage(error: Throwable): String {
            val e = PwmError.from(error)
            return MESSAGES[e.code] ?: Messages.byCode[e.code] ?: e.userMessage
        }
    }
}
