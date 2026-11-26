package net.haven.app.pictures

import android.graphics.Bitmap
import android.graphics.BitmapFactory

/** Android's decoder: reads the size first, decodes at a power-of-two
 *  reduction close to the box, then scales to fit exactly. */
object AndroidPictures : PictureCodec<Bitmap> {
    override fun size(bytes: ByteArray): Pair<Int, Int>? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        return if (bounds.outWidth > 0 && bounds.outHeight > 0) bounds.outWidth to bounds.outHeight else null
    }

    override fun shrink(bytes: ByteArray, box: PictureSize): Bitmap? {
        val (width, height) = size(bytes) ?: return null
        val (wantWidth, wantHeight) = PictureSize.fit(width, height, box)
        var sample = 1
        while (width / (sample * 2) >= wantWidth && height / (sample * 2) >= wantHeight) sample *= 2
        val decoded = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample }) ?: return null
        if (decoded.width == wantWidth && decoded.height == wantHeight) return decoded
        return Bitmap.createScaledBitmap(decoded, wantWidth, wantHeight, true).also { if (it !== decoded) decoded.recycle() }
    }

    override fun cost(image: Bitmap): Int = image.allocationByteCount
}
