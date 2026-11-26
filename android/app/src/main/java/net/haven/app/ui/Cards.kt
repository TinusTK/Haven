package net.haven.app.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import net.haven.app.pictures.Pause
import net.haven.app.pictures.PictureSize
import net.haven.app.pictures.Pictures

/** The picture cache the screens load from (set by the app at start). */
val LocalPictures = staticCompositionLocalOf<Pictures<Bitmap>?> { null }

/** A picture from an address, loaded (and kept) by [Pictures]; `empty`
 *  shows until it arrives, or if it never does. */
@Composable
fun RemotePicture(
    url: String?,
    size: PictureSize,
    modifier: Modifier = Modifier,
    pause: Pause? = null,
    contentScale: ContentScale = ContentScale.Crop,
    loaded: () -> Unit = {},
    empty: @Composable BoxScope.() -> Unit = {},
) {
    val pictures = LocalPictures.current
    var image by remember(url) { mutableStateOf<Bitmap?>(null) }
    LaunchedEffect(url) {
        if (url == null || pictures == null) return@LaunchedEffect
        val found = withContext(Dispatchers.IO) { pictures.load(url, size, pause) }
        if (found != null) {
            image = found
            loaded()
        }
    }
    Box(modifier, contentAlignment = Alignment.Center) {
        val shown = image
        if (shown != null) {
            Image(shown.asImageBitmap(), null, Modifier.fillMaxSize(), contentScale = contentScale)
        } else {
            empty()
        }
    }
}

/** Watched: a gold tick in a round pill. */
@Composable
fun WatchedTick(size: Dp = 22.dp) {
    val palette = LocalPalette.current
    Box(
        Modifier.size(size).background(Color(0xCC11131A), CircleShape).border(1.5.dp, palette.accent, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        HavenIcon("check", size * 0.64f, palette.accent, stroke = 2.5.dp)
    }
}

/** A thin gold bar for how far something was watched. */
@Composable
fun ProgressBar(share: Double, modifier: Modifier = Modifier, track: Color = Color.White.copy(alpha = 0.25f)) {
    Box(modifier.fillMaxWidth().height(4.dp).clip(RoundedCornerShape(50)).background(track)) {
        Box(Modifier.fillMaxWidth(share.coerceIn(0.0, 1.0).toFloat()).height(4.dp).clip(RoundedCornerShape(50)).background(LocalPalette.current.accent))
    }
}

/** Poster sizes that suit the width: three across a phone, more across a
 *  tablet (posters about 160 dp wide there). */
object PosterFit {
    /** Wider than this, the page is laid out for a tablet. */
    const val WIDE = 700f

    /** Columns and card width for a grid `width` wide (its padding of 16 a
     *  side and gaps of 10 already taken off). */
    fun grid(width: Float, tv: Boolean = false): Pair<Int, Float> {
        val inside = maxOf(width - 32, 1f)
        val count = if (width < WIDE) 3 else maxOf(3, ((inside + 10) / if (tv) TV_CELL else 170f).toInt())
        return count to maxOf((inside - 10 * (count - 1)) / count, 1f)
    }

    /** On a TV, as Apple TV's 230 and 440 points: a poster 120 dp wide,
     *  a wide card 220 dp, and grid cells of about 130 dp. */
    const val TV_POSTER = 120f
    const val TV_WIDE = 220f
    const val TV_CELL = 130f

    /** A poster in a row `width` wide: 3.15 across a phone. */
    fun row(width: Float): Float = if (width < WIDE) maxOf((width - 24) / 3.15f, 1f) else 160f
}

/** A poster on a shelf: the picture (or the title when there is none), its
 *  score in a dark pill, a gold tick when watched, a bar for how far it was
 *  watched, and the title under it. */
@Composable
fun PosterCard(title: String, poster: String?, width: Dp, modifier: Modifier = Modifier, rating: String? = null, watched: Boolean = false, progress: Double = 0.0, pause: Pause? = null) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(10.dp)
    Column(modifier.width(width), horizontalAlignment = Alignment.CenterHorizontally) {
        Box(Modifier.size(width, width * 1.5f).clip(shape).background(palette.surface).border(1.dp, palette.border, shape)) {
            RemotePicture(poster, PictureSize.Poster, Modifier.fillMaxSize(), pause) {
                Text(title, Modifier.padding(horizontal = 10.dp), style = Type.font(13.sp, color = palette.textDim), textAlign = TextAlign.Center)
            }
            Column(Modifier.fillMaxSize().padding(7.dp)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    if (!rating.isNullOrEmpty()) {
                        Box(Modifier.height(24.dp).background(Color(0xC70A0A0A), RoundedCornerShape(7.dp)).padding(horizontal = 8.dp), contentAlignment = Alignment.Center) {
                            Text("★ $rating", style = Type.font(12.sp, FontWeight.Bold, Color.White))
                        }
                    } else if (watched) {
                        WatchedTick(26.dp)
                    }
                }
                Box(Modifier.weight(1f))
                if (watched && !rating.isNullOrEmpty()) Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) { WatchedTick(26.dp) }
                if (progress > 0 && !watched) ProgressBar(progress, Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp), Color.Black.copy(alpha = 0.63f))
            }
        }
        Text(title, Modifier.padding(top = 7.dp), style = Type.font(14.sp, FontWeight.SemiBold, palette.text), maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** A wide card (Upcoming, Continue Watching): a 16:9 picture with a badge
 *  and pills, and two lines under it. */
@Composable
fun WideCard(
    title: String,
    line: String,
    picture: String?,
    badge: String,
    width: Dp,
    modifier: Modifier = Modifier,
    progress: Double? = null,
    pause: Pause? = null,
    corner: @Composable BoxScope.() -> Unit = {},
) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(16.dp)
    Column(modifier.width(width)) {
        Box(Modifier.size(width, width * 0.5625f).clip(shape).background(palette.surface).border(1.dp, palette.border, shape)) {
            RemotePicture(picture, PictureSize.Wide, Modifier.fillMaxSize(), pause)
            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0.5f to Color.Transparent, 1f to Color.Black.copy(alpha = 0.62f))))
            if (badge.isNotEmpty()) {
                Text(badge, Modifier.align(Alignment.BottomStart).padding(start = 16.dp, bottom = if (progress == null) 14.dp else 18.dp), style = Type.font(12.sp, FontWeight.ExtraBold, Color.White))
            }
            corner()
            if (progress != null) ProgressBar(progress, Modifier.align(Alignment.BottomCenter).padding(start = 12.dp, end = 12.dp, bottom = 8.dp))
        }
        Text(title, Modifier.padding(top = 10.dp), style = Type.font(15.sp, FontWeight.SemiBold, palette.text), maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(line, Modifier.padding(top = 4.dp), style = Type.font(13.sp, color = palette.textDim), maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** A row's heading with an arrow that opens the page behind it. */
@Composable
fun RowHeading(title: String, action: (() -> Unit)? = null) {
    val palette = LocalPalette.current
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(title, style = Type.heading(20.sp, palette.text))
        if (action != null) {
            Box(Modifier.size(26.dp).pressable(RoundedCornerShape(6.dp), onClick = action), contentAlignment = Alignment.Center) {
                HavenIcon("forward", 18.dp, palette.textDim)
            }
        }
    }
}
