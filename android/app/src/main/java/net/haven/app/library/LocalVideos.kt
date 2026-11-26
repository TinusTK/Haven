package net.haven.app.library

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.provider.MediaStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** The device's videos, from Android's media list, newest first (at most
 *  1000). Needs the right to read videos, asked for when Local opens. */
object LocalVideos {
    val permission: String
        get() = if (Build.VERSION.SDK_INT >= 33) android.Manifest.permission.READ_MEDIA_VIDEO else android.Manifest.permission.READ_EXTERNAL_STORAGE

    fun allowed(context: Context) = context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    suspend fun find(context: Context): List<FoundVideo> = withContext(Dispatchers.IO) {
        val base = MediaStore.Video.Media.EXTERNAL_CONTENT_URI
        val columns = arrayOf(MediaStore.Video.Media._ID, MediaStore.Video.Media.DISPLAY_NAME, MediaStore.Video.Media.SIZE,
            MediaStore.Video.Media.DATE_MODIFIED, MediaStore.Video.Media.BUCKET_DISPLAY_NAME)
        val found = mutableListOf<FoundVideo>()
        context.contentResolver.query(base, columns, null, null, "${MediaStore.Video.Media.DATE_MODIFIED} DESC")?.use { cursor ->
            while (found.size < DownloadRules.MOST_VIDEOS && cursor.moveToNext()) {
                val name = cursor.getString(1) ?: ""
                found += FoundVideo(
                    name.substringBeforeLast('.', name), cursor.getString(4) ?: "", maxOf(cursor.getLong(2), 0),
                    cursor.getLong(3), "$base/${cursor.getLong(0)}",
                )
            }
        }
        found
    }
}
