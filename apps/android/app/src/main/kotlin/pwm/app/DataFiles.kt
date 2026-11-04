package pwm.app

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import pwm.kit.ImportFile
import pwm.kit.PwmError
import pwm.kit.ui.ImportFiles

/**
 * The export and import files (D59 §6-7), through the system's pickers:
 * the bytes go straight to or from the file picked, with no copy of ours.
 */
object DataFiles {
    /** Writes an export to the file the user picked with "Save to". */
    fun write(context: Context, uri: Uri, bytes: ByteArray): Boolean = KitFiles.write(context, uri, bytes)
}

/** The content resolver for files picked to import; a `file:` address only in tests. */
class AndroidImportFiles(context: Context) : ImportFiles {
    private val resolver = context.applicationContext.contentResolver

    override fun name(uri: String): String? {
        val parsed = Uri.parse(uri)
        if (parsed.scheme == "file") return parsed.lastPathSegment
        return query(parsed, OpenableColumns.DISPLAY_NAME) { cursor, i -> cursor.getString(i) }
    }

    override fun size(uri: String): Long? {
        val parsed = Uri.parse(uri)
        if (parsed.scheme == "file") return parsed.path?.let { File(it).length() }
        return query(parsed, OpenableColumns.SIZE) { cursor, i -> if (cursor.isNull(i)) null else cursor.getLong(i) }
    }

    override fun read(uri: String): ByteArray = try {
        val stream = resolver.openInputStream(Uri.parse(uri)) ?: throw PwmError("import_unreadable")
        stream.use { input ->
            // Read no further than the limit, whatever the size said.
            val bytes = input.readNBytesCompat(ImportFile.MAX_BYTES.toInt() + 1)
            if (bytes.size > ImportFile.MAX_BYTES) throw PwmError("import_too_large")
            bytes
        }
    } catch (e: FileNotFoundException) {
        throw PwmError("import_unreadable")
    } catch (e: IOException) {
        throw PwmError("import_unreadable")
    } catch (e: SecurityException) {
        throw PwmError("import_unreadable")
    }

    /** Where the file came from deletes it; some places (cloud drives) can't. */
    override fun delete(uri: String): Boolean {
        val parsed = Uri.parse(uri)
        return try {
            when (parsed.scheme) {
                "file" -> parsed.path?.let { File(it).delete() } ?: false
                else -> DocumentsContract.deleteDocument(resolver, parsed)
            }
        } catch (e: FileNotFoundException) {
            false
        } catch (e: SecurityException) {
            false
        } catch (e: UnsupportedOperationException) {
            false
        } catch (e: IllegalArgumentException) {
            // Not a document address.
            false
        }
    }

    private fun <T> query(uri: Uri, column: String, read: (android.database.Cursor, Int) -> T?): T? = try {
        resolver.query(uri, arrayOf(column), null, null, null)?.use { cursor ->
            val i = cursor.getColumnIndex(column)
            if (i >= 0 && cursor.moveToFirst()) read(cursor, i) else null
        }
    } catch (e: SecurityException) {
        null
    } catch (e: IllegalArgumentException) {
        null
    }

    private fun java.io.InputStream.readNBytesCompat(limit: Int): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        val buffer = ByteArray(64 * 1024)
        while (out.size() < limit) {
            val n = read(buffer, 0, minOf(buffer.size, limit - out.size()))
            if (n < 0) break
            out.write(buffer, 0, n)
        }
        return out.toByteArray()
    }
}
