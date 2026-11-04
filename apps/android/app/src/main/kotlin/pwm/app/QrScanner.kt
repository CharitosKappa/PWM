package pwm.app

import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Button
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import java.util.concurrent.Executors
import pwm.kit.QrDecoder
import pwm.kit.design.Tokens

/**
 * The camera, looking for a QR code (D53 §3, D54 §6): the Emergency Kit's,
 * or a site's one-time-code setup. CameraX shows it and hands frames to
 * ZXing, all on the device. [onCode] gets each code found and returns true
 * when it was the one wanted, which closes the camera.
 */
@Composable
fun QrScanner(hint: String, onCode: (String) -> Boolean, onClose: () -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val found by rememberUpdatedState(onCode)
    val frames = remember { Executors.newSingleThreadExecutor() }
    val provider = remember { ProcessCameraProvider.getInstance(context) }
    DisposableEffect(Unit) {
        onDispose {
            if (provider.isDone) provider.get().unbindAll()
            frames.shutdown()
        }
    }
    Box(Modifier.fillMaxSize().testTag("scanner")) {
        AndroidView(
            factory = { ctx ->
                val view = PreviewView(ctx)
                provider.addListener({
                    val cameras = provider.get()
                    val preview = Preview.Builder().build().also { it.setSurfaceProvider(view.surfaceProvider) }
                    val analysis = ImageAnalysis.Builder()
                        .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                        .build()
                    analysis.setAnalyzer(frames) { image ->
                        val text = image.use(::decode)
                        if (text != null) view.post { if (found(text)) cameras.unbindAll() }
                    }
                    cameras.unbindAll()
                    cameras.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
                }, ContextCompat.getMainExecutor(ctx))
                view
            },
            modifier = Modifier.fillMaxSize(),
        )
        Surface(color = color(Tokens.Color.bg), modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth()) {
            Column(
                Modifier.safeDrawingPadding().padding(Tokens.Space.s5.dp),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.s3.dp),
            ) {
                Text(hint, style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
                Button(onClick = onClose, modifier = Modifier.fillMaxWidth().testTag("closeScanner")) { Text(stringResource(R.string.cancel)) }
            }
        }
    }
}

/** The frame's brightness plane, read by ZXing; null when it holds no QR code. */
private fun decode(image: ImageProxy): String? {
    val plane = image.planes.firstOrNull() ?: return null
    val buffer = plane.buffer
    val bytes = ByteArray(buffer.remaining()).also { buffer.get(it) }
    return runCatching { QrDecoder.decode(bytes, image.width, image.height, plane.rowStride) }.getOrNull()
}

private fun <T> ImageProxy.use(block: (ImageProxy) -> T): T = try {
    block(this)
} finally {
    close()
}
