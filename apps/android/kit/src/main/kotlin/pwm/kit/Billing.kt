package pwm.kit

import uniffi.pwm_ffi.BillingInfo

/**
 * The account's plan, as the server has it (D6 §7). The server is the one
 * source of the plan: bought anywhere, it shows everywhere.
 */
data class BillingStatus(
    /** `free`, `premium`, `family` or `teams`. */
    val plan: String,
    /** The subscription that gives the plan, if any. */
    val subscription: Subscription?,
    /** Who sells plans on this server (`stripe`, `app_store`, `play`); empty, every account is Premium. */
    val sells: List<String>,
    /** The account's billing token, Play's `obfuscatedAccountId`; only from [Account.billingStatus]. */
    val token: Redacted<String>?,
) {
    /** Times are Unix ms. */
    data class Subscription(
        /** `stripe`, `app_store` or `play`: where it's managed. */
        val provider: String,
        val plan: String,
        /** `active`, `canceled`, `past_due` or `expired`. */
        val status: String,
        /** The end of the paid period. */
        val untilMs: Long,
        /** It ends at [untilMs] instead of renewing. */
        val canceling: Boolean,
        val priceLockedUntilMs: Long?,
        /** Until when a refund can be asked for here (D72 §1): Stripe and Play, 30 days after the last charge; null otherwise. */
        val refundableUntilMs: Long?,
        /** Other subscriptions that count too, bought by mistake: where to cancel each. */
        val also: List<String>,
    )

    companion object {
        const val FREE = "free"
        const val STRIPE = "stripe"
        const val APP_STORE = "app_store"
        const val PLAY = "play"

        internal fun of(info: BillingInfo): BillingStatus = BillingStatus(
            plan = info.plan,
            subscription = info.subscription?.let {
                Subscription(it.provider, it.plan, it.status, it.untilMs, it.canceling, it.priceLockedUntilMs, it.refundableUntilMs, it.also)
            },
            sells = info.sells,
            token = info.token?.let(::Redacted),
        )
    }
}

/** The Play Console's products (D6 §9). One constant until D1 and D8 name the product and its price. */
object PlayProducts {
    /** The Premium subscription; a placeholder id. */
    const val PREMIUM = "premium_yearly"
}
