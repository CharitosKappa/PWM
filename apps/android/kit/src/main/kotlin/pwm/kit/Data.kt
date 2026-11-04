package pwm.kit

import uniffi.pwm_ffi.ImportMapping
import uniffi.pwm_ffi.ImportPreview

// The data half of Settings (D59 §5-7): the health report, export and
// import, as the core answers them. As on Apple (D43 §7-8, D51).

/** Weak passwords (item id to the strength score) and groups of logins sharing one. Local only. */
class PasswordHealth(val weak: Map<String, Int>, val reused: List<List<String>>) {
    override fun toString(): String = "PasswordHealth(${weak.size} weak, ${reused.size} reused)"
}

/** What an export writes (D24). */
sealed class ExportFormat {
    /** Encrypted with its own password; opens without the account. */
    class Pwmx(val password: Redacted<String>) : ExportFormat()

    data object Json : ExportFormat()

    data object Csv : ExportFormat()

    val fileExtension: String get() = when (this) {
        is Pwmx -> "pwmx"
        Json -> "json"
        Csv -> "csv"
    }

    /** For the system's "Save to". */
    val mimeType: String get() = when (this) {
        is Pwmx -> "application/octet-stream"
        Json -> "application/json"
        Csv -> "text/csv"
    }

    internal val core: uniffi.pwm_ffi.ExportFormat get() = when (this) {
        is Pwmx -> uniffi.pwm_ffi.ExportFormat.Pwmx(password.value)
        Json -> uniffi.pwm_ffi.ExportFormat.Json
        Csv -> uniffi.pwm_ffi.ExportFormat.Csv
    }
}

/** Importing another manager's export, or our own (D51). */
object ImportFile {
    /** The formats the core reads, by the ids it uses; the screen has the labels. */
    val FORMATS = listOf("bitwarden", "1password", "keeper", "keepass", "nordpass", "protonpass", "dashlane", "lastpass", "csv", "pwm")

    /** The core's own limit (D51 §7). */
    const val MAX_BYTES = 50L * 1024 * 1024

    /** A KeePass key file. */
    const val MAX_KEY_FILE_BYTES = 1024L * 1024
}

/** What a picked file looks like. */
data class ImportGuess(val format: String, val needsPassword: Boolean)

/** Which CSV column (by index) holds each field; null leaves it out. */
data class ColumnMap(
    val title: Int? = null,
    val url: Int? = null,
    val username: Int? = null,
    val password: Int? = null,
    val notes: Int? = null,
    val totp: Int? = null,
    val folder: Int? = null,
    val favorite: Int? = null,
    val tags: Int? = null,
) {
    /** The fields the column picker offers. */
    enum class Field { TITLE, URL, USERNAME, PASSWORD, NOTES, TOTP, FOLDER }

    operator fun get(field: Field): Int? = when (field) {
        Field.TITLE -> title
        Field.URL -> url
        Field.USERNAME -> username
        Field.PASSWORD -> password
        Field.NOTES -> notes
        Field.TOTP -> totp
        Field.FOLDER -> folder
    }

    fun with(field: Field, column: Int?): ColumnMap = when (field) {
        Field.TITLE -> copy(title = column)
        Field.URL -> copy(url = column)
        Field.USERNAME -> copy(username = column)
        Field.PASSWORD -> copy(password = column)
        Field.NOTES -> copy(notes = column)
        Field.TOTP -> copy(totp = column)
        Field.FOLDER -> copy(folder = column)
    }

    internal val core: ImportMapping get() = ImportMapping(
        title?.toUInt(), url?.toUInt(), username?.toUInt(), password?.toUInt(), notes?.toUInt(),
        totp?.toUInt(), folder?.toUInt(), favorite?.toUInt(), tags?.toUInt(),
    )

    internal companion object {
        fun of(m: ImportMapping) = ColumnMap(
            m.title?.toInt(), m.url?.toInt(), m.username?.toInt(), m.password?.toInt(), m.notes?.toInt(),
            m.totp?.toInt(), m.folder?.toInt(), m.favorite?.toInt(), m.tags?.toInt(),
        )
    }
}

/** A CSV's header row, with the suggested columns. Header names aren't secrets; rows are never kept. */
data class CsvColumns(val headers: List<String>, val suggested: ColumnMap, val rows: Int)

/** What an import brings in, and what it leaves out. */
data class ImportSummary(
    val logins: Int,
    val notes: Int,
    val cards: Int,
    val identities: Int,
    val folders: Int,
    /** Of the above, how many come in the trash. */
    val trashed: Int,
    val leftOut: List<LeftOut>,
) {
    /** `attachments`, `ssh_keys`, `aliases`, `passkeys`, `unknown_types`, `empty`, `too_large` or `duplicates`. */
    data class LeftOut(val reason: String, val count: Int)

    internal companion object {
        fun of(p: ImportPreview) = ImportSummary(
            p.logins.toInt(), p.notes.toInt(), p.cards.toInt(), p.identities.toInt(), p.folders.toInt(), p.trashed.toInt(),
            p.leftOut.map { LeftOut(it.reason, it.count.toInt()) },
        )
    }
}
