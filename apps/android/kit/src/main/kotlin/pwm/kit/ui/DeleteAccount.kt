package pwm.kit.ui

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Account
import pwm.kit.BillingStatus
import pwm.kit.Redacted

/**
 * Deleting the account (D74 §7), from Settings: what will happen, with
 * export offered first; the plan, when there is one; then the master
 * password. No typed word and no countdown: the password is the
 * confirmation.
 */
class DeleteAccountModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    private val onDeleted: () -> Unit,
) {
    enum class Step { INTRO, PLAN, PASSWORD }

    data class State(
        val step: Step = Step.INTRO,
        /** Who bills the plan, and the other subscriptions that count too: `stripe`, `play`, `app_store`. */
        val billedBy: List<String> = emptyList(),
        val password: Redacted<String> = Redacted(""),
        val busy: Boolean = false,
        val error: String? = null,
    ) {
        /** Stripe or Google Play: the plan ends with the unused days back. */
        val endsWithRefund: Boolean get() = billedBy.any { it == BillingStatus.STRIPE || it == BillingStatus.PLAY }

        /** Only Apple can end an App Store plan. */
        val appleBills: Boolean get() = BillingStatus.APP_STORE in billedBy

        val canDelete: Boolean get() = !busy && password.value.isNotEmpty()
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    /** Reads the plan; without it the plan step is skipped, and the server still ends a Stripe or Play plan first. */
    fun load() {
        scope.launch {
            val subscription = try {
                account.billingStatus().subscription
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                null
            }
            val billedBy = subscription?.let { listOf(it.provider) + it.also } ?: emptyList()
            mutable.update { it.copy(billedBy = billedBy) }
        }
    }

    fun next() = mutable.update {
        when (it.step) {
            Step.INTRO -> it.copy(step = if (it.billedBy.isEmpty()) Step.PASSWORD else Step.PLAN)
            Step.PLAN, Step.PASSWORD -> it.copy(step = Step.PASSWORD)
        }
    }

    fun setPassword(value: String) = mutable.update { it.copy(password = Redacted(value), error = null) }

    /** Proves the password to the server and deletes everything; this device is wiped and the app goes back to its first screen. */
    fun delete() {
        val s = mutable.value
        if (!s.canDelete) return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                account.deleteAccount(s.password)
                mutable.update { it.copy(password = Redacted(""), busy = false) }
                onDeleted()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(password = Redacted(""), busy = false, error = message(e)) }
            }
        }
    }
}
