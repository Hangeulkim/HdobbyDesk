package com.hdobby.hdobbydesk

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.Toast
import androidx.annotation.Keep
import com.google.protobuf.ByteString
import ffi.FFI
import hbb.MessageOuterClass.Clipboard
import hbb.MessageOuterClass.ClipboardFormat
import hbb.MessageOuterClass.MultiClipboards
import java.nio.ByteBuffer
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

class RdClipboardManager(
    private val context: Context,
    private val clipboardManager: ClipboardManager,
    private val sendToRemote: (ByteBuffer) -> Unit = { FFI.onClipboardUpdate(it) }
) {
    private val logTag = "RdClipboardManager"
    private val main = Handler(Looper.getMainLooper())
    // Keep image decoding and provider reads off the UI thread, with bounded pending work.
    private val worker = ThreadPoolExecutor(1, 1, 30, TimeUnit.SECONDS,
        ArrayBlockingQueue<Runnable>(1), ThreadPoolExecutor.DiscardOldestPolicy()).apply {
        allowCoreThreadTimeOut(true)
    }
    private var lastUpdatedClipData: ClipData? = null
    private var isClientEnabled = true
    private var revision = 0L
    @Volatile private var _isCaptureStarted = false
    val isCaptureStarted: Boolean get() = _isCaptureStarted

    private fun onMain(block: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) block() else main.post { block() }
    }

    // Compare items independently of MIME indices: one item may have several representations.
    private fun isClipboardDataEqual(left: ClipData, right: ClipData): Boolean {
        if (left.itemCount != right.itemCount || left.description.mimeTypeCount != right.description.mimeTypeCount) return false
        for (i in 0 until left.description.mimeTypeCount) {
            if (left.description.getMimeType(i) != right.description.getMimeType(i)) return false
        }
        for (i in 0 until left.itemCount) {
            val a = left.getItemAt(i)
            val b = right.getItemAt(i)
            if (a.text?.toString() != b.text?.toString() || a.htmlText != b.htmlText ||
                a.uri != b.uri || a.intent?.toUri(0) != b.intent?.toUri(0)) return false
        }
        return true
    }

    private fun reportFailure() {
        // Do not log clipboard contents, provider URIs or filenames.
        Log.w(logTag, "Clipboard operation failed or format/size is unsupported")
        Toast.makeText(context, "클립보드를 전송하지 못했습니다. 파일은 ‘파일 전송’을 사용해 주세요.", Toast.LENGTH_LONG).show()
    }

    fun checkPrimaryClip(isClient: Boolean) = onMain {
        if (isClient && !isClientEnabled) return@onMain
        val clipData = try { clipboardManager.primaryClip } catch (_: SecurityException) { null }
        if (clipData == null || clipData.itemCount == 0) return@onMain
        if (isClient && lastUpdatedClipData?.let { isClipboardDataEqual(clipData, it) } == true) return@onMain
        val request = ++revision
        worker.execute {
            try {
                // Multi-file clipboard uses file transfer; never silently send only the first file.
                require(clipData.itemCount == 1) { "Use file transfer for multiple clipboard items" }
                val item = clipData.getItemAt(0)
                val clips = MultiClipboards.newBuilder()
                fun add(format: ClipboardFormat, bytes: ByteArray) {
                    require(bytes.size <= ClipboardImageCodec.MAX_BYTES)
                    clips.addClipboards(Clipboard.newBuilder().setFormat(format).setContent(ByteString.copyFrom(bytes)))
                }
                item.text?.let { add(ClipboardFormat.Text, it.toString().toByteArray(Charsets.UTF_8)) }
                item.htmlText?.let { add(ClipboardFormat.Html, it.toByteArray(Charsets.UTF_8)) }
                item.uri?.let { uri ->
                    require(uri.scheme == "content") { "Only granted content providers may supply images" }
                    val type = context.contentResolver.getType(uri)
                    require(type?.startsWith("image/") == true) { "Use file transfer for files" }
                    val bytes = requireNotNull(context.contentResolver.openInputStream(uri)).use { ClipboardImageCodec.readBounded(it) }
                    add(ClipboardFormat.ImagePng, ClipboardImageCodec.asPng(bytes))
                }
                require(clips.clipboardsCount > 0)
                val bytes = clips.build().toByteArray()
                main.post {
                    if (request == revision && (!isClient || isClientEnabled)) {
                        val buffer = ByteBuffer.allocateDirect(bytes.size + 1)
                        buffer.put(if (isClient) 1.toByte() else 0.toByte()).put(bytes).flip()
                        sendToRemote(buffer)
                        lastUpdatedClipData = clipData
                    }
                }
            } catch (_: Exception) {
                main.post { if (request == revision) reportFailure() }
            }
        }
    }

    fun setCaptureStarted(started: Boolean) { _isCaptureStarted = started }

    @Keep
    fun rustEnableClientClipboard(enable: Boolean) = onMain {
        isClientEnabled = enable
        revision++
        lastUpdatedClipData = null
    }

    fun syncClipboard(isClient: Boolean) { checkPrimaryClip(isClient) }

    @Keep
    fun rustUpdateClipboard(bytes: ByteArray) = onMain {
        val request = ++revision
        worker.execute {
            try {
                // Native Rust has already decompressed the protobuf representations.
                require(bytes.size <= 80 * 1024 * 1024)
                val clips = MultiClipboards.parseFrom(bytes)
                var text: String? = null
                var html: String? = null
                var png: ByteArray? = null
                val pngClip = clips.clipboardsList.firstOrNull { it.format == ClipboardFormat.ImagePng }
                if (pngClip != null) png = ClipboardImageCodec.asPng(pngClip.content.toByteArray())
                for (clip in clips.clipboardsList) {
                    when (clip.format) {
                        ClipboardFormat.Text -> text = clip.content.toStringUtf8()
                        ClipboardFormat.Html -> html = clip.content.toStringUtf8()
                        ClipboardFormat.ImageRgba -> if (png == null) {
                            png = ClipboardImageCodec.rgbaToPng(clip.content.toByteArray(), clip.width, clip.height)
                        }
                        else -> Unit
                    }
                }
                val imageUri = png?.let { ClipboardImageCodec.publish(context, it) }
                require(text != null || imageUri != null) { "Unsupported clipboard content" }
                val mimeTypes = mutableListOf<String>()
                if (text != null) mimeTypes.add(ClipDescription.MIMETYPE_TEXT_PLAIN)
                if (html != null && text != null) mimeTypes.add(ClipDescription.MIMETYPE_TEXT_HTML)
                if (imageUri != null) mimeTypes.add("image/png")
                val item = ClipData.Item(text, if (text != null) html else null, null, imageUri)
                val clipData = ClipData(ClipDescription("Remote clipboard", mimeTypes.toTypedArray()), item)
                main.post {
                    if (request == revision) {
                        try {
                            clipboardManager.setPrimaryClip(clipData)
                            lastUpdatedClipData = clipData
                        } catch (_: Exception) { reportFailure() }
                    }
                }
            } catch (_: Exception) {
                main.post { if (request == revision) reportFailure() }
            }
        }
    }
}
