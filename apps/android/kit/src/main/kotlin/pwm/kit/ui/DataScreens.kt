package pwm.kit.ui

import java.time.LocalDate
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import pwm.kit.Account
import pwm.kit.ColumnMap
import pwm.kit.CsvColumns
import pwm.kit.ExportFormat
import pwm.kit.ImportFile
import pwm.kit.ImportSummary
import pwm.kit.PasswordStrength
import pwm.kit.PwmError
import pwm.kit.Redacted

// Settings' data screens (D59 §5-7), as on Apple (D43 §7-8, D51 §9).

/**
 * Weak, reused and leaked passwords (D22, D44). The leak check runs when it
 * is switched on in Settings; its results stay in memory for this screen.
 */
class HealthModel internal constructor(
    private val scope: CoroutineScope,
    private val vault: VaultModel,
    private val breachCheckOn: Boolean,
    private val onEdit: (Entry) -> Unit,
) {
    class Leaked(val entry: Entry, val count: Long) {
        override fun toString(): String = "Leaked($count)"
    }

    data class State(
        val weak: List<Entry> = emptyList(),
        val reused: List<List<Entry>> = emptyList(),
        val leaked: List<Leaked> = emptyList(),
        val loaded: Boolean = false,
        val checkingLeaks: Boolean = false,
        /** The leak check ran in this report. */
        val checkedLeaks: Boolean = false,
        val error: String? = null,
    ) {
        val isClean: Boolean get() = loaded && !checkingLeaks && weak.isEmpty() && reused.isEmpty() && leaked.isEmpty()
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable
    private var loading: Job? = null

    /** The local checks first, then the leak check if it is on. Again when the screen shows again. */
    fun load() {
        if (loading?.isActive == true) return
        loading = scope.launch { run() }
    }

    private suspend fun run() {
        vault.load()
        val byId = vault.state.value.entries.filter { it.item.trashedAtMs == null }.associateBy { it.id }
        try {
            val health = vault.account.passwordHealth()
            val weak = health.weak.keys.mapNotNull(byId::get).sortedWith(VaultModel.ORDER)
            val reused = health.reused
                .map { group -> group.mapNotNull(byId::get).sortedWith(VaultModel.ORDER) }
                .filter { it.size > 1 }
            mutable.update { it.copy(weak = weak, reused = reused, loaded = true, error = null) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(error = message(e)) }
            return
        }
        if (!breachCheckOn) return
        mutable.update { it.copy(checkingLeaks = true) }
        try {
            val counts = vault.account.breachedPasswords()
            val leaked = counts.mapNotNull { (id, count) -> byId[id]?.let { Leaked(it, count) } }
                .sortedWith(compareBy(VaultModel.ORDER) { it.entry })
            mutable.update { it.copy(leaked = leaked, checkedLeaks = true) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(error = message(e)) }
        } finally {
            mutable.update { it.copy(checkingLeaks = false) }
        }
    }

    /** Fixing an item opens its edit screen on the vault tab, with Generate. */
    fun edit(entry: Entry) = onEdit(entry)
}

/**
 * Export (D24): the master password every time, then an encrypted file with
 * its own password, or a plaintext one after a warning. Every plan. The
 * file goes to the system's "Save to"; its bytes stay only until saved.
 */
class ExportModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    private val work: CoroutineDispatcher,
    private val today: () -> LocalDate = LocalDate::now,
) {
    enum class Choice { ENCRYPTED, JSON, CSV }

    data class State(
        val choice: Choice = Choice.ENCRYPTED,
        val masterPassword: Redacted<String> = Redacted(""),
        val exportPassword: Redacted<String> = Redacted(""),
        val confirm: Redacted<String> = Redacted(""),
        /** The plaintext warning was read. */
        val understood: Boolean = false,
        val strength: PasswordStrength? = null,
        /** The file, made and waiting for "Save to". */
        val file: Redacted<ByteArray>? = null,
        val fileName: String = "",
        val mimeType: String = "",
        val saved: Boolean = false,
        val busy: Boolean = false,
        val error: String? = null,
    ) {
        val passwordsMatch: Boolean get() = confirm.value.isNotEmpty() && exportPassword.value == confirm.value
        val strongEnough: Boolean get() = (strength?.score ?: 0) >= CreateAccountModel.MINIMUM_SCORE

        val canExport: Boolean get() = !busy && masterPassword.value.isNotEmpty() && when (choice) {
            Choice.ENCRYPTED -> strongEnough && passwordsMatch
            Choice.JSON, Choice.CSV -> understood
        }
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    fun setChoice(choice: Choice) = mutable.update { it.copy(choice = choice, error = null) }

    fun setMasterPassword(value: String) = mutable.update { it.copy(masterPassword = Redacted(value), error = null) }

    fun setExportPassword(value: String) {
        val password = Redacted(value)
        mutable.update { it.copy(exportPassword = password, strength = null) }
        if (value.isEmpty()) return
        scope.launch {
            val scored = withContext(work) { PasswordStrength.of(password) }
            mutable.update { if (it.exportPassword.value == value) it.copy(strength = scored) else it }
        }
    }

    fun setConfirm(value: String) = mutable.update { it.copy(confirm = Redacted(value)) }

    fun setUnderstood(understood: Boolean) = mutable.update { it.copy(understood = understood) }

    /** Checks the master password, then makes the file for "Save to". */
    fun export() {
        val s = mutable.value
        if (!s.canExport) return
        val format = when (s.choice) {
            Choice.ENCRYPTED -> ExportFormat.Pwmx(s.exportPassword)
            Choice.JSON -> ExportFormat.Json
            Choice.CSV -> ExportFormat.Csv
        }
        mutable.update { it.copy(busy = true, error = null, saved = false) }
        scope.launch {
            try {
                account.authorizeExport(s.masterPassword)
                val file = account.export(format)
                mutable.update {
                    it.copy(
                        masterPassword = Redacted(""), exportPassword = Redacted(""), confirm = Redacted(""), strength = null,
                        file = file, fileName = "PWM export ${today()}.${format.fileExtension}", mimeType = format.mimeType,
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(masterPassword = Redacted(""), error = message(e)) }
            } finally {
                mutable.update { it.copy(busy = false) }
            }
        }
    }

    /** The file was written where the user picked: the bytes go. */
    fun didSave() = mutable.update { it.copy(file = null, saved = true, understood = false, error = null) }

    /** Writing failed; the file stays for another try. */
    fun saveFailed() = mutable.update { it.copy(error = message(PwmError("export_save_failed"))) }

    /** "Save to" was cancelled, or the screen went: the bytes go. */
    fun done() = mutable.update { it.copy(file = null, understood = false) }
}

/**
 * Import in Settings (D51 §9, D59 §7): pick a file, check where it's from,
 * enter its password if it has one, see what will come in, then import.
 * After a file with no password, one line says it holds passwords in plain
 * text, with a button to delete it. No master password: importing only adds.
 */
class ImportModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    private val files: ImportFiles,
    private val work: CoroutineDispatcher,
) {
    data class State(
        val fileName: String? = null,
        val format: String = "",
        val password: Redacted<String> = Redacted(""),
        /** A KeePass key file's name, once picked. */
        val keyFileName: String? = null,
        val needsPassword: Boolean = false,
        /** For a generic CSV. */
        val columns: CsvColumns? = null,
        val columnMap: ColumnMap = ColumnMap(),
        /** What "Import" will bring in. */
        val summary: ImportSummary? = null,
        /** What came in. */
        val done: ImportSummary? = null,
        /** The imported file had no password: it holds passwords in plain text. */
        val plaintext: Boolean = false,
        val deleted: Boolean = false,
        val working: Boolean = false,
        val error: String? = null,
        internal val hasFile: Boolean = false,
        internal val hasUri: Boolean = false,
    ) {
        /** The password field shows for encrypted exports and the formats that may have one. */
        val asksPassword: Boolean get() = needsPassword || format in setOf("bitwarden", "keepass", "pwm")

        /** KeePass databases may also need a key file. */
        val asksKeyFile: Boolean get() = format == "keepass"

        val canCheck: Boolean get() = hasFile && format.isNotEmpty() && !working

        val canDelete: Boolean get() = plaintext && hasUri && !deleted
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    // The picked file, in memory only; never in [State].
    private var data: Redacted<ByteArray>? = null
    private var uri: String? = null
    private var keyFile: Redacted<ByteArray>? = null

    /** The user picked a file: it is read into memory once; one over the limit is refused before reading. */
    fun picked(uri: String) {
        reset()
        mutable.update { it.copy(working = true) }
        scope.launch {
            try {
                val name = withContext(work) { files.name(uri) } ?: "export"
                val size = withContext(work) { files.size(uri) }
                if (size != null && size > ImportFile.MAX_BYTES) throw PwmError("import_too_large")
                val bytes = Redacted(withContext(work) { files.read(uri) })
                if (bytes.value.size > ImportFile.MAX_BYTES) throw PwmError("import_too_large")
                data = bytes
                this@ImportModel.uri = uri
                val guess = account.guessImport(name, bytes)
                val format = guess?.format ?: ""
                val columns = if (format == "csv") withContext(work) { account.importColumns(bytes) } else null
                mutable.update {
                    it.copy(
                        fileName = name, format = format, needsPassword = guess?.needsPassword ?: false,
                        columns = columns, columnMap = columns?.suggested ?: ColumnMap(), hasFile = true, hasUri = true,
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                data = null
                this@ImportModel.uri = null
                mutable.update { it.copy(error = message(e)) }
            } finally {
                mutable.update { it.copy(working = false) }
            }
        }
    }

    /** The user picked a KeePass key file (1 MB at most). */
    fun pickedKeyFile(uri: String) {
        mutable.update { it.copy(summary = null, error = null) }
        scope.launch {
            try {
                val size = withContext(work) { files.size(uri) }
                if (size == null || size > ImportFile.MAX_KEY_FILE_BYTES) throw PwmError("import_unreadable")
                val bytes = withContext(work) { files.read(uri) }
                if (bytes.size > ImportFile.MAX_KEY_FILE_BYTES) throw PwmError("import_unreadable")
                keyFile = Redacted(bytes)
                val name = withContext(work) { files.name(uri) } ?: "key"
                mutable.update { it.copy(keyFileName = name) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(PwmError("import_unreadable"))) }
            }
        }
    }

    fun setFormat(format: String) = mutable.update { if (it.format == format) it else it.copy(format = format, summary = null) }

    fun setPassword(value: String) = mutable.update { it.copy(password = Redacted(value), summary = null) }

    fun setColumn(field: ColumnMap.Field, column: Int?) = mutable.update {
        val map = it.columnMap.with(field, column)
        if (map == it.columnMap) it else it.copy(columnMap = map, summary = null)
    }

    /** Reads the file as the chosen format and shows what will come in. */
    fun check() {
        val s = mutable.value
        val data = data ?: return
        if (!s.canCheck) return
        mutable.update { it.copy(working = true, error = null) }
        scope.launch {
            try {
                val summary = account.prepareImport(
                    s.format, data,
                    password = s.password.takeIf { it.value.isNotEmpty() },
                    keyFile = keyFile.takeIf { s.format == "keepass" },
                    columns = s.columnMap.takeIf { s.format == "csv" },
                )
                mutable.update { it.copy(summary = summary) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(summary = null, error = message(e)) }
            } finally {
                mutable.update { it.copy(working = false) }
            }
        }
    }

    /** Saves what [check] showed; the file's bytes are dropped. */
    fun importNow() {
        val s = mutable.value
        if (s.summary == null || s.working) return
        mutable.update { it.copy(working = true, error = null) }
        scope.launch {
            try {
                val done = account.finishImport()
                val plaintext = s.password.value.isEmpty() && keyFile == null
                data = null
                keyFile = null
                mutable.update {
                    it.copy(done = done, plaintext = plaintext, summary = null, password = Redacted(""), keyFileName = null, hasFile = false)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            } finally {
                mutable.update { it.copy(working = false) }
            }
        }
    }

    /** Deletes the picked export file; offered after a plaintext import. */
    fun deleteFile() {
        val uri = uri ?: return
        if (!mutable.value.canDelete) return
        scope.launch {
            val deleted = try {
                withContext(work) { files.delete(uri) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                false
            }
            if (deleted) {
                this@ImportModel.uri = null
                mutable.update { it.copy(deleted = true, hasUri = false, error = null) }
            } else {
                mutable.update { it.copy(error = message(PwmError("delete_failed"))) }
            }
        }
    }

    /** Leaving the screen, or locking, drops the file and any preview. */
    fun close() {
        val hadPreview = mutable.value.summary != null
        data = null
        keyFile = null
        mutable.update { it.copy(password = Redacted(""), summary = null, hasFile = false) }
        if (hadPreview) scope.launch { account.cancelImport() }
    }

    private fun reset() {
        data = null
        uri = null
        keyFile = null
        mutable.value = State()
    }

    companion object {
        /** "2 logins, 1 card", or null for nothing new. */
        fun counts(s: ImportSummary): List<Pair<Kind, Int>> = listOf(
            Kind.LOGINS to s.logins, Kind.NOTES to s.notes, Kind.CARDS to s.cards,
            Kind.IDENTITIES to s.identities, Kind.FOLDERS to s.folders,
        ).filter { it.second > 0 }
    }

    /** What the counts are of; the screen has the words. */
    enum class Kind { LOGINS, NOTES, CARDS, IDENTITIES, FOLDERS }
}
