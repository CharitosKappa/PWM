package pwm.app

import android.content.Context
import android.net.Uri
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import java.io.FileOutputStream
import java.io.IOException

/**
 * Saving and printing the Emergency Kit (D53 §2). The PDF bytes go straight
 * to the file the user picked, or to the print system: never a temporary
 * file of ours.
 */
object KitFiles {
    /** Writes the kit to a file the user picked with the system's "Save to". */
    fun write(context: Context, uri: Uri, pdf: ByteArray): Boolean = try {
        context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(pdf) } != null
    } catch (e: IOException) {
        false
    } catch (e: SecurityException) {
        false
    }

    /** Opens Android's print dialog with the kit. */
    fun print(context: Context, jobName: String, pdf: ByteArray, onWritten: () -> Unit, onFailed: () -> Unit) {
        val manager = context.getSystemService(PrintManager::class.java) ?: return onFailed()
        manager.print(jobName, KitPrint(jobName, pdf, onWritten, onFailed), null)
    }

    private class KitPrint(
        private val name: String,
        private val pdf: ByteArray,
        private val onWritten: () -> Unit,
        private val onFailed: () -> Unit,
    ) : PrintDocumentAdapter() {
        override fun onLayout(
            oldAttributes: PrintAttributes?,
            newAttributes: PrintAttributes,
            cancellationSignal: CancellationSignal,
            callback: LayoutResultCallback,
            extras: Bundle?,
        ) {
            if (cancellationSignal.isCanceled) return callback.onLayoutCancelled()
            val info = PrintDocumentInfo.Builder(name)
                .setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT)
                .setPageCount(1)
                .build()
            callback.onLayoutFinished(info, oldAttributes != newAttributes)
        }

        override fun onWrite(
            pages: Array<out PageRange>,
            destination: ParcelFileDescriptor,
            cancellationSignal: CancellationSignal,
            callback: WriteResultCallback,
        ) {
            if (cancellationSignal.isCanceled) return callback.onWriteCancelled()
            try {
                FileOutputStream(destination.fileDescriptor).use { it.write(pdf) }
                callback.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
                onWritten()
            } catch (e: IOException) {
                callback.onWriteFailed(null)
                onFailed()
            }
        }
    }
}
