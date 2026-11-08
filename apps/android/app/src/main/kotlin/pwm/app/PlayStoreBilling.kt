package pwm.app

import android.app.Activity
import android.content.Context
import com.android.billingclient.api.BillingClient
import com.android.billingclient.api.BillingClient.BillingResponseCode
import com.android.billingclient.api.BillingClient.ProductType
import com.android.billingclient.api.BillingClientStateListener
import com.android.billingclient.api.BillingFlowParams
import com.android.billingclient.api.BillingResult
import com.android.billingclient.api.PendingPurchasesParams
import com.android.billingclient.api.ProductDetails
import com.android.billingclient.api.Purchase
import com.android.billingclient.api.PurchasesUpdatedListener
import com.android.billingclient.api.QueryProductDetailsParams
import com.android.billingclient.api.QueryPurchasesParams
import kotlin.coroutines.resume
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import pwm.kit.Redacted
import pwm.kit.ui.PlayBilling
import pwm.kit.ui.PlayPurchase
import pwm.kit.ui.PlayResult

/**
 * Google Play Billing (D6 §2, §10) with the Play Billing Library. The
 * purchase is linked to the account by its billing token as
 * `obfuscatedAccountId`; the server reads it from Google and acknowledges
 * it, so this never calls `acknowledgePurchase`. Purchases and tokens are
 * never logged. Called on the main thread, where the library answers too.
 */
class PlayStoreBilling(context: Context, private val activity: () -> Activity?) : PlayBilling, PurchasesUpdatedListener {
    override val packageName: String = context.packageName

    private val client: BillingClient = BillingClient.newBuilder(context.applicationContext)
        .setListener(this)
        // Required by the library; a subscription bought with a slow payment method arrives as PENDING.
        .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
        .build()

    private val later = MutableSharedFlow<PlayResult>(extraBufferCapacity = 8)
    override val results: Flow<PlayResult> = later

    private val connecting = Mutex()

    /** The answer [buy] waits for, while Play's sheet shows. */
    private var waiting: CompletableDeferred<PlayResult>? = null

    override suspend fun buy(productId: String, accountToken: Redacted<String>): PlayResult {
        val shown = activity() ?: return PlayResult.Failed
        if (!connected()) return PlayResult.Unavailable
        val details = productDetails(productId) ?: return PlayResult.Unavailable
        // The base plan: no introductory offers (PRICING.md §3, D6 §5).
        val offers = details.subscriptionOfferDetails.orEmpty()
        val offer = offers.firstOrNull { it.offerId == null } ?: offers.firstOrNull() ?: return PlayResult.Unavailable
        val params = BillingFlowParams.newBuilder()
            .setProductDetailsParamsList(
                listOf(BillingFlowParams.ProductDetailsParams.newBuilder().setProductDetails(details).setOfferToken(offer.offerToken).build()),
            )
            .setObfuscatedAccountId(accountToken.value)
            .build()
        val answer = CompletableDeferred<PlayResult>()
        waiting = answer
        try {
            val launched = client.launchBillingFlow(shown, params)
            if (launched.responseCode != BillingResponseCode.OK) return result(launched, null)
            return answer.await()
        } finally {
            if (waiting === answer) waiting = null
        }
    }

    override suspend fun owned(): List<PlayPurchase> {
        if (!connected()) return emptyList()
        val params = QueryPurchasesParams.newBuilder().setProductType(ProductType.SUBS).build()
        return suspendCancellableCoroutine { done ->
            client.queryPurchasesAsync(params) { result, purchases ->
                done.resume(if (result.responseCode == BillingResponseCode.OK) purchases.map(::purchase) else emptyList())
            }
        }
    }

    /** Play's answer to the sheet, or a purchase that changed later (a pending payment that went through). */
    override fun onPurchasesUpdated(result: BillingResult, purchases: List<Purchase>?) {
        val answer = result(result, purchases)
        val sheet = waiting
        if (sheet != null && !sheet.isCompleted) {
            waiting = null
            sheet.complete(answer)
        } else if (answer is PlayResult.Purchases) {
            later.tryEmit(answer)
        }
    }

    private suspend fun connected(): Boolean = connecting.withLock {
        if (client.isReady) return@withLock true
        suspendCancellableCoroutine { done ->
            client.startConnection(
                object : BillingClientStateListener {
                    override fun onBillingSetupFinished(result: BillingResult) {
                        if (done.isActive) done.resume(result.responseCode == BillingResponseCode.OK)
                    }

                    // Connected again on the next call.
                    override fun onBillingServiceDisconnected() {
                        if (done.isActive) done.resume(false)
                    }
                },
            )
        }
    }

    private suspend fun productDetails(productId: String): ProductDetails? {
        val product = QueryProductDetailsParams.Product.newBuilder().setProductId(productId).setProductType(ProductType.SUBS).build()
        val params = QueryProductDetailsParams.newBuilder().setProductList(listOf(product)).build()
        return suspendCancellableCoroutine { done ->
            client.queryProductDetailsAsync(params) { result, found ->
                val details = if (result.responseCode == BillingResponseCode.OK) found.productDetailsList.firstOrNull() else null
                done.resume(details)
            }
        }
    }

    private fun result(result: BillingResult, purchases: List<Purchase>?): PlayResult = when (result.responseCode) {
        BillingResponseCode.OK -> PlayResult.Purchases(purchases.orEmpty().map(::purchase))
        BillingResponseCode.USER_CANCELED -> PlayResult.Canceled
        BillingResponseCode.ITEM_ALREADY_OWNED -> PlayResult.AlreadyOwned
        BillingResponseCode.BILLING_UNAVAILABLE, BillingResponseCode.SERVICE_UNAVAILABLE, BillingResponseCode.SERVICE_DISCONNECTED,
        BillingResponseCode.FEATURE_NOT_SUPPORTED, BillingResponseCode.ITEM_UNAVAILABLE -> PlayResult.Unavailable
        else -> PlayResult.Failed
    }

    private fun purchase(p: Purchase): PlayPurchase = PlayPurchase(
        productIds = p.products,
        token = Redacted(p.purchaseToken),
        state = when (p.purchaseState) {
            Purchase.PurchaseState.PURCHASED -> PlayPurchase.State.PURCHASED
            Purchase.PurchaseState.PENDING -> PlayPurchase.State.PENDING
            else -> PlayPurchase.State.UNSPECIFIED
        },
        accountToken = p.accountIdentifiers?.obfuscatedAccountId?.let(::Redacted),
        acknowledged = p.isAcknowledged,
    )
}
