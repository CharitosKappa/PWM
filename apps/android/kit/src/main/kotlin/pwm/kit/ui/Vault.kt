package pwm.kit.ui

import java.text.Collator
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Account
import pwm.kit.CurrentCode
import pwm.kit.Item
import pwm.kit.PwmError
import pwm.kit.Redacted
import pwm.kit.SyncResult

// The vault screens' state (D54 §1-4), as on Apple (D43 §2-4): the list with
// its sections and search, an item's detail, and editing. Items are opened
// by the core and held here only while the vault is unlocked; locking drops
// these models.

/** An item and the vault it lives in. */
class Entry(val vaultId: String, val item: Item) {
    val id: String get() = item.id ?: ""

    override fun toString(): String = "Entry(<redacted>)"
}

/** The list's sections (D43 §2). */
sealed class Section {
    data object All : Section()

    data object Favorites : Section()

    data class OfKind(val kind: Item.Kind) : Section()

    data class Folder(val id: String) : Section()

    data object Trash : Section()

    companion object {
        /** The type sections, in order. */
        val KINDS = listOf(Item.Kind.LOGIN, Item.Kind.CARD, Item.Kind.IDENTITY, Item.Kind.NOTE)
    }
}

/** Why the list shows a notice; the screen has the words. */
enum class SyncNotice { SOME_FAILED, OFFLINE, SIGNED_OUT }

/** The vault tab: the list, its sections and search, and the item and edit screens on top. */
class VaultModel internal constructor(
    private val scope: CoroutineScope,
    internal val account: Account,
    internal val clipboard: SecretClipboard,
    internal val work: CoroutineDispatcher,
    private val onSignedOut: () -> Unit,
    /** A sync was told the session ended: "Sign in again to sync" (D50 §5). */
    private val onSessionEnded: () -> Unit = {},
) {
    /** The screens above the list, the last one showing. */
    sealed class Route {
        class Detail(val model: ItemDetailModel) : Route()

        class Edit(val model: ItemEditModel) : Route()
    }

    data class State(
        val entries: List<Entry> = emptyList(),
        val section: Section = Section.All,
        val query: String = "",
        /** What the section and search show, in order. */
        val visible: List<Entry> = emptyList(),
        /** Folders not in the trash, by name. */
        val folders: List<Entry> = emptyList(),
        val loaded: Boolean = false,
        val syncing: Boolean = false,
        val notice: SyncNotice? = null,
        val error: String? = null,
        val stack: List<Route> = emptyList(),
        /** Back was pressed on an edit with changes: ask before discarding. */
        val discardAsked: Boolean = false,
    )

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable
    private var syncAgain = false

    fun setSection(section: Section) = mutable.update { it.copy(section = section).filtered() }

    fun setQuery(query: String) = mutable.update { it.copy(query = query).filtered() }

    private fun State.filtered(): State = copy(visible = filter(entries, section, query), folders = foldersOf(entries))

    // --- Loading and sync ---

    /** Reads every vault from the cache on this device. */
    suspend fun load() {
        try {
            val all = account.vaultIds().flatMap { vault -> account.items(vault).map { Entry(vault, it) } }
            mutable.update { it.copy(entries = all, loaded = true, error = null).filtered() }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(error = message(e)) }
        }
    }

    /**
     * Syncs, empties old trash and reloads. A sync already running runs
     * once more when it ends, so no change waits for the next refresh.
     */
    fun refresh() {
        if (mutable.value.syncing) {
            syncAgain = true
            return
        }
        mutable.update { it.copy(syncing = true) }
        scope.launch {
            try {
                do {
                    syncAgain = false
                    if (!syncOnce()) return@launch
                } while (syncAgain)
            } finally {
                mutable.update { it.copy(syncing = false) }
            }
        }
    }

    /** False when the device was signed out. */
    private suspend fun syncOnce(): Boolean {
        try {
            val notice = when (val result = account.sync()) {
                is SyncResult.Synced -> if (result.failed > 0) SyncNotice.SOME_FAILED else null
                is SyncResult.Retry -> SyncNotice.OFFLINE
                SyncResult.SignedOut -> {
                    onSessionEnded()
                    null
                }
                SyncResult.DeviceRevoked -> {
                    // The core has cleared the cache; finish signing out.
                    runCatching { account.signOut() }
                    onSignedOut()
                    return false
                }
            }
            mutable.update { it.copy(notice = notice) }
            account.purgeTrash()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(error = message(e)) }
        }
        load()
        return true
    }

    // --- Moving between screens (D54 §3) ---

    fun open(entry: Entry) = push(Route.Detail(ItemDetailModel(entry, account, clipboard)))

    /** What a list row copies without opening the item: its TalkBack actions. */
    enum class RowCopy { PASSWORD, USERNAME, CODE }

    fun copy(field: RowCopy, entry: Entry) {
        scope.launch {
            val value = when (field) {
                RowCopy.PASSWORD -> entry.item.password
                RowCopy.USERNAME -> entry.item.username
                RowCopy.CODE -> try {
                    account.currentCode(entry.id, entry.vaultId)?.code?.value
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    null
                }
            }
            if (value.isNullOrEmpty()) return@launch
            clipboard.copy(Redacted(value), SecretKeyModel.CLIPBOARD_MS)
            Speech.say(Announcement.COPIED)
        }
    }

    fun edit(entry: Entry) = push(Route.Edit(ItemEditModel(scope, entry, isNew = false, vault = this)))

    /**
     * A new item of [kind], not saved until the user saves it. It goes into
     * the folder being shown, and is a favorite when that is shown.
     */
    fun newItem(kind: Item.Kind) {
        scope.launch {
            try {
                val vault = account.vaultIds().firstOrNull() ?: throw PwmError("unknown_vault")
                val section = mutable.value.section
                val item = account.newItem(kind, "").edited {
                    if (section is Section.Folder) folderId = section.id
                    if (section == Section.Favorites) favorite = true
                }
                push(Route.Edit(ItemEditModel(scope, Entry(vault, item), isNew = true, vault = this@VaultModel)))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
        }
    }

    private fun push(route: Route) = mutable.update { it.copy(stack = it.stack + route, error = null) }

    /**
     * The back gesture: an edit with changes asks first; otherwise the top
     * screen goes. False on the list itself.
     */
    fun back(): Boolean {
        val top = mutable.value.stack.lastOrNull() ?: return false
        if (top is Route.Edit && top.model.hasChanges && !mutable.value.discardAsked) {
            mutable.update { it.copy(discardAsked = true) }
            return true
        }
        pop()
        return true
    }

    fun confirmDiscard() = pop()

    fun keepEditing() = mutable.update { it.copy(discardAsked = false) }

    private fun pop() {
        (mutable.value.stack.lastOrNull() as? Route.Detail)?.model?.hideAll()
        mutable.update { it.copy(stack = it.stack.dropLast(1), discardAsked = false) }
    }

    /** Back to the list, masking whatever was shown. */
    fun closeAll() {
        mutable.value.stack.forEach { (it as? Route.Detail)?.model?.hideAll() }
        mutable.update { it.copy(stack = emptyList(), discardAsked = false) }
    }

    /** After a save: show it now (the edit closes), sync in the background. */
    internal suspend fun saved(itemId: String, vaultId: String) {
        load()
        mutable.update { state ->
            val stack = state.stack.dropLast(1).map { route ->
                if (route is Route.Detail && route.model.entry.id == itemId) {
                    val fresh = state.entries.firstOrNull { it.id == itemId && it.vaultId == vaultId }
                    if (fresh != null) Route.Detail(ItemDetailModel(fresh, account, clipboard)) else route
                } else {
                    route
                }
            }
            state.copy(stack = stack, discardAsked = false)
        }
        refresh()
    }

    // --- Trash (D43 §2) ---

    fun trash(entry: Entry) = change { account.setTrashed(entry.id, entry.vaultId, trashed = true) }

    fun restore(entry: Entry) = change { account.setTrashed(entry.id, entry.vaultId, trashed = false) }

    /** Deletes a trashed item for good; the screen asks first. */
    fun deleteForever(entry: Entry) = change { account.delete(entry.id, entry.vaultId) }

    private fun change(action: suspend () -> Unit) {
        scope.launch {
            try {
                action()
                closeAll()
                load()
                refresh()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
        }
    }

    internal companion object {
        // --- Search (D43 §3, the same rule on every platform) ---

        fun filter(entries: List<Entry>, section: Section, query: String): List<Entry> {
            val needle = query.trim()
            return entries
                .filter { it.item.kind != Item.Kind.FOLDER && inSection(it.item, section) }
                .filter { needle.isEmpty() || matches(it.item, needle) }
                .sortedWith(ORDER)
        }

        fun foldersOf(entries: List<Entry>): List<Entry> =
            entries.filter { it.item.kind == Item.Kind.FOLDER && it.item.trashedAtMs == null }.sortedWith(ORDER)

        private fun inSection(item: Item, section: Section): Boolean {
            val trashed = item.trashedAtMs != null
            return when (section) {
                Section.Trash -> trashed
                Section.All -> !trashed
                Section.Favorites -> !trashed && item.favorite
                is Section.OfKind -> !trashed && item.kind == section.kind
                is Section.Folder -> !trashed && item.folderId == section.id
            }
        }

        /** A case-insensitive substring of the title, username, a URL or a card's last 4 digits. */
        private fun matches(item: Item, needle: String): Boolean {
            val fields = mutableListOf(item.title)
            if (item.kind == Item.Kind.LOGIN) {
                fields += item.username
                fields += item.urls.map { it.url }
            }
            item.card?.lastFour?.takeIf { it.isNotEmpty() }?.let(fields::add)
            return fields.any { it.contains(needle, ignoreCase = true) }
        }

        /** By title in the user's language, then by id, so the order is stable. */
        internal val ORDER: Comparator<Entry> = run {
            val collator = Collator.getInstance().apply { strength = Collator.SECONDARY }
            Comparator { a, b -> collator.compare(a.item.title, b.item.title).takeIf { it != 0 } ?: a.id.compareTo(b.id) }
        }
    }
}

/**
 * One item's fields: hidden values stay masked until tapped, and are masked
 * again when the screen goes (D43 §2).
 */
class ItemDetailModel internal constructor(
    val entry: Entry,
    private val account: Account,
    private val clipboard: SecretClipboard,
) {
    data class State(
        /** Keys of the hidden fields the user revealed: `password`, `number`, `securityCode`, `field.<n>`, `history.<n>`. */
        val revealed: Set<String> = emptySet(),
        val code: CurrentCode? = null,
        /** The login's code is a kind this version can't make (HOTP, say). */
        val codeUnsupported: Boolean = false,
        /** The plan doesn't include one-time codes (D73 §1); the secret stays readable. */
        val codeNeedsPlan: Boolean = false,
        /** The field copied last, for its "Copied" label. */
        val copied: String? = null,
    )

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable
    val item: Item get() = entry.item

    fun toggle(field: String) = mutable.update {
        it.copy(revealed = if (field in it.revealed) it.revealed - field else it.revealed + field)
    }

    fun copy(value: String, field: String) {
        clipboard.copy(Redacted(value), SecretKeyModel.CLIPBOARD_MS)
        mutable.update { it.copy(copied = field) }
        Speech.say(Announcement.COPIED)
    }

    fun copyCode() {
        val code = mutable.value.code ?: return
        copy(code.code.value, "code")
    }

    /** Reads the current one-time code; the screen calls it every second while it shows. */
    suspend fun refreshCode() {
        if (item.totp.isNullOrEmpty()) return
        var needsPlan = false
        val code = try {
            account.currentCode(entry.id, entry.vaultId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            needsPlan = PwmError.from(e).code == "plan_required"
            null
        }
        mutable.update { it.copy(code = code, codeUnsupported = code == null && !needsPlan, codeNeedsPlan = needsPlan) }
    }

    /** Masks everything again, when the screen goes away. */
    fun hideAll() = mutable.update { it.copy(revealed = emptySet(), code = null) }

    companion object {
        /** Display text for a hidden value: a `•` for each character, 8 to 12 of them. */
        fun masked(value: String): String = "•".repeat(value.length.coerceIn(8, 12))

        /** A one-time code in two groups ("123 456"), shown and read that way. */
        fun grouped(code: String): String =
            if (code.length <= 4) code else code.take(code.length / 2) + " " + code.drop(code.length / 2)
    }
}
