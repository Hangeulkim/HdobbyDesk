package com.hdobby.hdobbydesk

import android.app.Instrumentation
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.os.Bundle
import com.google.protobuf.ByteString
import hbb.MessageOuterClass.Clipboard
import hbb.MessageOuterClass.ClipboardFormat
import hbb.MessageOuterClass.MultiClipboards
import java.io.ByteArrayInputStream
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Local OS clipboard integration; transport is captured, so this is not a remote-session test. */
class ClipboardInstrumentation : Instrumentation() {
    override fun onCreate(arguments: Bundle?) { super.onCreate(arguments); start() }

    override fun onStart() {
        val results = mutableListOf<String>()
        try {
            val app = targetContext
            val intent = requireNotNull(app.packageManager.getLaunchIntentForPackage(app.packageName))
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            startActivitySync(intent)
            waitForIdleSync()
            val clipboard = app.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            val sent = LinkedBlockingQueue<ByteArray>()
            val manager = RdClipboardManager(app, clipboard) { buffer ->
                val bytes = ByteArray(buffer.remaining())
                buffer.get(bytes)
                sent.offer(bytes)
            }
            fun checkCase(name: String, block: () -> Unit) { block(); results.add(name) }
            fun current(): ClipData? {
                var clip: ClipData? = null
                runOnMainSync { clip = clipboard.primaryClip }
                return clip
            }
            fun awaitClip(matches: (ClipData) -> Boolean): ClipData {
                val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(8)
                while (System.nanoTime() < deadline) {
                    val clip = current()
                    if (clip != null && matches(clip)) return clip
                    Thread.sleep(40)
                }
                error("Clipboard update timed out")
            }
            fun update(format: ClipboardFormat, bytes: ByteArray, width: Int = 0, height: Int = 0) {
                val clip = Clipboard.newBuilder().setFormat(format).setContent(ByteString.copyFrom(bytes))
                    .setWidth(width).setHeight(height)
                manager.rustUpdateClipboard(MultiClipboards.newBuilder().addClipboards(clip).build().toByteArray())
            }
            val sample = "English 한글 조합 😀\n둘째 줄\t123"
            checkCase("receive_unicode_text") {
                update(ClipboardFormat.Text, sample.toByteArray())
                awaitClip { it.getItemAt(0).text?.toString() == sample }
            }
            checkCase("send_unicode_and_html") {
                runOnMainSync { clipboard.setPrimaryClip(ClipData.newHtmlText("test", sample, "<b>$sample</b>")) }
                manager.syncClipboard(true)
                val bytes = requireNotNull(sent.poll(8, TimeUnit.SECONDS))
                check(bytes[0] == 1.toByte())
                val clips = MultiClipboards.parseFrom(bytes.copyOfRange(1, bytes.size))
                check(clips.clipboardsList.first { it.format == ClipboardFormat.Text }.content.toStringUtf8() == sample)
                check(clips.clipboardsList.first { it.format == ClipboardFormat.Html }.content.toStringUtf8() == "<b>$sample</b>")
            }
            val rgba = byteArrayOf(-1, 0, 0, -1, 0, -1, 0, -1)
            val png = ClipboardImageCodec.rgbaToPng(rgba, 2, 1)
            checkCase("rgba_color_order_and_dimensions") {
                val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size)
                check(bitmap.width == 2 && bitmap.height == 1)
                check(bitmap.getPixel(0, 0) == 0xffff0000.toInt())
                check(bitmap.getPixel(1, 0) == 0xff00ff00.toInt())
                bitmap.recycle()
            }
            checkCase("receive_png_and_read_provider_bytes") {
                update(ClipboardFormat.ImagePng, png)
                val clip = awaitClip { it.description.hasMimeType("image/png") }
                val uri = requireNotNull(clip.getItemAt(0).uri)
                val bytes = requireNotNull(app.contentResolver.openInputStream(uri)).use { it.readBytes() }
                check(bytes.contentEquals(png))
            }
            checkCase("received_image_does_not_echo") {
                manager.syncClipboard(true)
                check(sent.poll(300, TimeUnit.MILLISECONDS) == null)
            }
            checkCase("send_local_image_uri") {
                val uri = ClipboardImageCodec.publish(app, png)
                runOnMainSync { clipboard.setPrimaryClip(ClipData.newUri(app.contentResolver, "local test", uri)) }
                manager.syncClipboard(true)
                val bytes = requireNotNull(sent.poll(8, TimeUnit.SECONDS))
                val clips = MultiClipboards.parseFrom(bytes.copyOfRange(1, bytes.size))
                check(clips.clipboardsList.single().format == ClipboardFormat.ImagePng)
                check(clips.clipboardsList.single().content.toByteArray().contentEquals(png))
            }
            checkCase("disabled_sync_does_not_send") {
                manager.rustEnableClientClipboard(false)
                runOnMainSync { clipboard.setPrimaryClip(ClipData.newPlainText("test", "disabled")) }
                manager.syncClipboard(true)
                check(sent.poll(300, TimeUnit.MILLISECONDS) == null)
            }
            checkCase("invalid_images_rejected") {
                check(runCatching { ClipboardImageCodec.asPng(byteArrayOf(1, 2, 3)) }.isFailure)
                check(runCatching { ClipboardImageCodec.rgbaToPng(byteArrayOf(1), 1, 1) }.isFailure)
                check(runCatching { ClipboardImageCodec.rgbaToPng(byteArrayOf(), Int.MAX_VALUE, Int.MAX_VALUE) }.isFailure)
                check(runCatching { ClipboardImageCodec.readBounded(ByteArrayInputStream(ByteArray(ClipboardImageCodec.MAX_BYTES + 1))) }.isFailure)
            }
            runOnMainSync { clipboard.setPrimaryClip(ClipData.newPlainText("test complete", "")) }
            finish(0, Bundle().apply { putString("stream", "PASS ${results.size}: ${results.joinToString()}\n") })
        } catch (error: Throwable) {
            finish(1, Bundle().apply { putString("stream", "FAIL after ${results.joinToString()}: ${error.javaClass.simpleName}: ${error.message}\n${error.stackTrace.take(5).joinToString("\n")}\n") })
        }
    }
}
