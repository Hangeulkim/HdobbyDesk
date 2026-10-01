package com.hdobby.hdobbydesk

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.core.content.FileProvider
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.util.UUID

/** PNG stays byte-for-byte intact; other raster formats are converted losslessly to PNG. */
object ClipboardImageCodec {
    const val MAX_BYTES = 32 * 1024 * 1024
    const val MAX_PIXELS = 16 * 1024 * 1024

    fun readBounded(input: InputStream): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (true) {
            val count = input.read(buffer)
            if (count == -1) break
            require(output.size().toLong() + count <= MAX_BYTES) { "Clipboard image is too large" }
            output.write(buffer, 0, count)
        }
        return output.toByteArray()
    }

    fun asPng(bytes: ByteArray): ByteArray {
        require(bytes.size <= MAX_BYTES) { "Clipboard image is too large" }
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        require(bounds.outWidth > 0 && bounds.outHeight > 0 &&
            bounds.outWidth.toLong() * bounds.outHeight <= MAX_PIXELS) { "Invalid clipboard image dimensions" }
        // Decode even PNG once to reject truncated/corrupt input before advertising it to other apps.
        val bitmap = requireNotNull(BitmapFactory.decodeByteArray(bytes, 0, bytes.size)) { "Invalid clipboard image" }
        return try {
            if (bounds.outMimeType == "image/png") bytes else encodePng(bitmap)
        } finally {
            bitmap.recycle()
        }
    }

    fun rgbaToPng(bytes: ByteArray, width: Int, height: Int): ByteArray {
        val pixels = width.toLong() * height
        require(width > 0 && height > 0 && pixels <= MAX_PIXELS && pixels * 4 == bytes.size.toLong()) {
            "Invalid clipboard RGBA dimensions"
        }
        val colors = IntArray(pixels.toInt()) { index ->
            val offset = index * 4
            ((bytes[offset + 3].toInt() and 255) shl 24) or
                ((bytes[offset].toInt() and 255) shl 16) or
                ((bytes[offset + 1].toInt() and 255) shl 8) or
                (bytes[offset + 2].toInt() and 255)
        }
        val bitmap = Bitmap.createBitmap(colors, width, height, Bitmap.Config.ARGB_8888)
        return try { encodePng(bitmap) } finally { bitmap.recycle() }
    }

    private fun encodePng(bitmap: Bitmap): ByteArray {
        val output = ByteArrayOutputStream()
        check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) { "Cannot encode clipboard image" }
        require(output.size() <= MAX_BYTES) { "Clipboard image is too large" }
        return output.toByteArray()
    }

    fun publish(context: Context, png: ByteArray): Uri {
        val directory = File(context.cacheDir, "hdobby-clipboard")
        check(directory.isDirectory || directory.mkdirs()) { "Cannot create clipboard cache" }
        val file = File(directory, "${UUID.randomUUID()}.png")
        file.outputStream().use { it.write(png) }
        // Only this app's generated cache is pruned. Keep recent copies usable by paste targets.
        directory.listFiles()?.filter { it != file && it.extension == "png" }
            ?.sortedByDescending { it.lastModified() }?.drop(7)?.forEach { it.delete() }
        return FileProvider.getUriForFile(context, "${context.packageName}.hdobby.clipboard", file)
    }
}
