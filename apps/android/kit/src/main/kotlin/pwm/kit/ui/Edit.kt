package pwm.kit.ui

import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Item
import pwm.kit.OneTimeCode
import pwm.kit.PwmError
import pwm.kit.Redacted

/**
 * A new or edited item (D43 §4). The form holds text; saving applies it to
 * the item's JSON in place, so fields a newer app wrote stay as they are
 * (D18 §2), and the core checks the result.
 */
class ItemEditModel internal constructor(
    private val scope: CoroutineScope,
    entry: Entry,
    val isNew: Boolean,
    private val vault: VaultModel,
) {
    /** A website row; [id] keeps rows apart while they are added and deleted. */
    data class UrlRow(val id: Long, val url: String, val match: String)

    /** A custom-field row. A null kind is one from a newer app version, kept as it is. */
    data class FieldRow(val id: Long, val name: String, val value: Redacted<String>, val kind: Item.CustomField.Kind?)

    data class State(
        val title: String,
        val notes: String,
        val favorite: Boolean,
        val folderId: String?,
        /** Comma-separated. */
        val tags: String,
        val username: String,
        val password: Redacted<String>,
        /** As typed or scanned; checked by the core on save. */
        val totp: Redacted<String>,
        val urls: List<UrlRow>,
        val cardholder: String,
        val number: Redacted<String>,
        val expiryMonth: String,
        val expiryYear: String,
        val securityCode: Redacted<String>,
        val brand: String,
        /** Identity fields by their D18 key. */
        val identity: Map<String, String>,
        /** One address line per line. */
        val address: String,
        val customFields: List<FieldRow>,
        val folders: List<Entry> = emptyList(),
        val busy: Boolean = false,
        val error: String? = null,
    )

    private val original: Item = entry.item
    val vaultId: String = entry.vaultId
    val kind: Item.Kind? get() = original.kind
    val isEditable: Boolean get() = original.isEditable

    private val ids = AtomicLong()
    private val mutable: MutableStateFlow<State>
    val state: StateFlow<State>

    init {
        val item = original
        val card = item.card
        val identity = item.identity
        mutable = MutableStateFlow(
            State(
                title = item.title,
                notes = item.notes,
                favorite = item.favorite,
                folderId = item.folderId,
                tags = item.tags.joinToString(", "),
                username = item.username,
                password = Redacted(item.password),
                totp = Redacted(item.totp ?: ""),
                urls = item.urls.map { UrlRow(ids.incrementAndGet(), it.url, it.match) },
                cardholder = card?.cardholder ?: "",
                number = Redacted(card?.number ?: ""),
                expiryMonth = card?.expiryMonth?.toString() ?: "",
                expiryYear = card?.expiryYear?.toString() ?: "",
                securityCode = Redacted(card?.securityCode ?: ""),
                brand = card?.brand ?: "",
                identity = Item.IDENTITY_KEYS.associateWith { key -> identity?.let { identityField(it, key) } ?: "" },
                address = identity?.addressLines?.joinToString("\n") ?: "",
                customFields = item.customFields.map { FieldRow(ids.incrementAndGet(), it.name, Redacted(it.value), it.kind) },
                folders = vault.state.value.folders,
            ),
        )
        state = mutable
    }

    /** Changes the form; the screen passes what was typed. */
    fun edit(change: (State) -> State) = mutable.update { change(it).copy(error = null) }

    /** Whether leaving should ask to discard. */
    val hasChanges: Boolean
        get() = runCatching { !assembled(checkingCode = false).sameFields(original) }.getOrDefault(true)

    val canSave: Boolean
        get() = mutable.value.let { !it.busy && isEditable && it.title.isNotBlank() && (isNew || hasChanges) }

    fun addUrl() = edit { it.copy(urls = it.urls + UrlRow(ids.incrementAndGet(), "", "domain")) }

    fun removeUrl(id: Long) = edit { s -> s.copy(urls = s.urls.filterNot { it.id == id }) }

    fun addField(kind: Item.CustomField.Kind) =
        edit { it.copy(customFields = it.customFields + FieldRow(ids.incrementAndGet(), "", Redacted(""), kind)) }

    fun removeField(id: Long) = edit { s -> s.copy(customFields = s.customFields.filterNot { it.id == id }) }

    /** A generator for the password field's sheet. */
    fun makeGenerator(): GeneratorModel = GeneratorModel(scope, vault.work, vault.clipboard)

    /** A password from the generator sheet. */
    fun use(generated: Redacted<String>) = edit { it.copy(password = generated) }

    /** Makes a folder now (it is saved at once) and puts the item in it. */
    fun newFolder(name: String) {
        val trimmed = name.trim()
        if (trimmed.isEmpty()) return
        scope.launch {
            try {
                val folder = vault.account.newItem(Item.Kind.FOLDER, trimmed)
                val id = vault.account.save(folder, vaultId)
                vault.load()
                mutable.update { it.copy(folders = vault.state.value.folders, folderId = id) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
        }
    }

    /** Saves locally, then syncs in the background; the edit screen closes. */
    fun save() {
        if (!canSave) return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                val item = assembled(checkingCode = true)
                val id = vault.account.save(item, vaultId)
                mutable.update { it.copy(busy = false) }
                vault.saved(id, vaultId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(busy = false, error = message(e)) }
            }
        }
    }

    /**
     * The item with the edits applied. Empty websites, lines and unnamed
     * empty fields are dropped; the code is checked by the core when saving.
     */
    internal fun assembled(checkingCode: Boolean): Item {
        val s = mutable.value
        val title = s.title.trim()
        val edited = original.edited {
            this.title = title
            notes = s.notes
            favorite = s.favorite
            folderId = s.folderId
            tags = s.tags.split(',').map(String::trim).filter(String::isNotEmpty)
            setCustomFields(
                s.customFields
                    .filterNot { it.name.isEmpty() && it.value.value.isEmpty() }
                    .map { Item.CustomField(it.name, it.value.value, it.kind) },
            )
            when (original.kind) {
                Item.Kind.LOGIN -> {
                    username = s.username
                    password = s.password.value
                    setUrls(s.urls.filter { it.url.isNotBlank() }.map { Item.Url(it.url.trim(), it.match) })
                    val code = s.totp.value.trim()
                    totp = when {
                        code.isEmpty() -> null
                        code == original.totp -> code
                        checkingCode -> OneTimeCode.normalize(code, title)
                        else -> code
                    }
                }
                Item.Kind.CARD -> {
                    cardholder = s.cardholder
                    number = s.number.value
                    expiryMonth = parseNumber(s.expiryMonth, 1..12, "invalid_expiry_month")
                    expiryYear = parseNumber(s.expiryYear, 2000..2099, "invalid_expiry_year")
                    securityCode = s.securityCode.value
                    brand = s.brand
                }
                Item.Kind.IDENTITY -> {
                    for ((key, value) in s.identity) setIdentity(key, value)
                    setAddressLines(s.address.lines().map(String::trim).filter(String::isNotEmpty))
                }
                Item.Kind.NOTE, Item.Kind.FOLDER, null -> Unit
            }
        }
        return if (edited.sameFields(original)) original else edited
    }

    private fun parseNumber(text: String, range: IntRange, code: String): Int? {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return null
        val value = trimmed.toIntOrNull()
        if (value == null || value !in range) throw PwmError(code)
        return value
    }

    private companion object {
        fun identityField(identity: Item.Identity, key: String): String = when (key) {
            "first_name" -> identity.firstName
            "middle_name" -> identity.middleName
            "last_name" -> identity.lastName
            "email" -> identity.email
            "phone" -> identity.phone
            "company" -> identity.company
            "city" -> identity.city
            "region" -> identity.region
            "postal_code" -> identity.postalCode
            "country" -> identity.country
            else -> ""
        }
    }
}
