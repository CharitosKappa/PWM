package pwm.kit

import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The kit scanner's decoding (D53 §6): a kit QR drawn as a camera frame reads back as the kit. */
class QrDecoderTest {
    private val kit = "pwm-kit:1?email=ada%40example.com&secret-key=A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA"

    /** The code drawn as a brightness plane, with [padding] bytes after each row, as cameras do. */
    private fun frame(text: String, size: Int, padding: Int): ByteArray {
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size)
        val stride = size + padding
        val plane = ByteArray(stride * size) { 0x55 }
        for (y in 0 until size) for (x in 0 until size) {
            plane[y * stride + x] = if (matrix[x, y]) 0x10 else 0xF0.toByte()
        }
        return plane
    }

    @Test
    fun aKitCodeReadsBackAndGoesToTheCore() {
        val text = QrDecoder.decode(frame(kit, 400, padding = 64), 400, 400, rowStride = 464)
        assertEquals(kit, text)
        val scanned = EmergencyKit.scan(text!!)
        assertEquals("ada@example.com", scanned.email)
        assertEquals("A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA", scanned.secretKey.value)
        assertNull(scanned.server)
    }

    @Test
    fun aFrameWithNoCodeGivesNothing() {
        val blank = ByteArray(320 * 240) { (it % 251).toByte() }
        assertNull(QrDecoder.decode(blank, 320, 240))
    }
}
