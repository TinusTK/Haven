package net.haven.app.ui

import androidx.compose.ui.unit.dp

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.TextUnit
import net.haven.app.R

/** Haven's colours, dark and light (the Rust build's Theme, as the Swift
 *  build's colour sets have them). */
@Immutable
data class Palette(
    val background: Color,
    val surface: Color,
    val border: Color,
    val hover: Color,
    val selected: Color,
    val text: Color,
    val textDim: Color,
    val accent: Color,
    /** Stands out on purpose: the "Report an issue" pill. */
    val alert: Color,
    val dark: Boolean,
) {
    /** Text on the gold accent (dark in both themes). */
    val onAccent get() = Color(0xFF171A22)

    companion object {
        val Dark = Palette(
            background = Color(0xFF0D0F14), surface = Color(0xFF151821), border = Color(0xFF232733),
            hover = Color(0xFF1C202B), selected = Color(0xFF262B3A), text = Color(0xFFEEF0F6),
            textDim = Color(0xFF9AA1B3), accent = Color(0xFFF2C66D), alert = Color(0xFFE5484D), dark = true,
        )
        val Light = Palette(
            background = Color(0xFFF3F4F7), surface = Color(0xFFFFFFFF), border = Color(0xFFDFE2EA),
            hover = Color(0xFFECEEF4), selected = Color(0xFFE1E6F5), text = Color(0xFF171A22),
            textDim = Color(0xFF5D6475), accent = Color(0xFFB9851A), alert = Color(0xFFD13438), dark = false,
        )
    }
}

/** Figtree for text, Pixelify Sans for headings. The fonts' digits were
 *  taken out (as in the Rust build), so numbers show in the system's font. */
object Type {
    val figtree = FontFamily(
        Font(R.font.figtree_regular, FontWeight.Normal),
        Font(R.font.figtree_medium, FontWeight.Medium),
        Font(R.font.figtree_semibold, FontWeight.SemiBold),
        Font(R.font.figtree_bold, FontWeight.Bold),
        Font(R.font.figtree_extrabold, FontWeight.ExtraBold),
    )
    val pixelify = FontFamily(
        Font(R.font.pixelifysans_medium, FontWeight.Medium),
        Font(R.font.pixelifysans_bold, FontWeight.Bold),
    )

    /** Body text. */
    fun font(size: TextUnit, weight: FontWeight = FontWeight.Normal, color: Color = Color.Unspecified) =
        TextStyle(fontFamily = figtree, fontSize = size, fontWeight = weight, color = color)

    /** Headings. */
    fun heading(size: TextUnit, color: Color = Color.Unspecified) =
        TextStyle(fontFamily = pixelify, fontSize = size, fontWeight = FontWeight.Bold, color = color)
}

val LocalPalette = staticCompositionLocalOf { Palette.Dark }

/** Whether Haven is on a TV (sizes grow, focus shows). */
val LocalTV = staticCompositionLocalOf { false }

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
fun HavenTheme(dark: Boolean, tv: Boolean, content: @Composable () -> Unit) {
    val margin = with(androidx.compose.ui.platform.LocalDensity.current) { TVScrolling.MARGIN.toPx() }
    val scrolling = androidx.compose.foundation.gestures.LocalBringIntoViewSpec.current
    CompositionLocalProvider(
        LocalPalette provides if (dark) Palette.Dark else Palette.Light,
        LocalTV provides tv,
        androidx.compose.foundation.gestures.LocalBringIntoViewSpec provides if (tv) remember(margin) { TVScrolling(margin) } else scrolling,
        content = content,
    )
}

/** How pages and rows scroll as the remote moves on a TV, as on Apple TV:
 *  only as far as needed to show what the remote is on, with a margin (so a
 *  row's heading above it, and the next card beside it, show too). Android
 *  TV's own way keeps the focused thing at a fixed spot, so every press
 *  slid the whole row or page. */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
class TVScrolling(private val margin: Float) : androidx.compose.foundation.gestures.BringIntoViewSpec {
    override fun calculateScrollDistance(offset: Float, size: Float, containerSize: Float): Float =
        TVScrolling.distance(offset, size, containerSize, margin)

    companion object {
        val MARGIN = 56.dp

        /** How far to scroll so a thing `size` long at `offset` shows in a
         *  container `containerSize` long, `margin` clear of its edges where
         *  there is room. 0 when it already shows. */
        fun distance(offset: Float, size: Float, containerSize: Float, margin: Float): Float {
            val room = maxOf(minOf(margin, (containerSize - size) / 2), 0f)
            val before = offset - room
            val after = offset + size + room - containerSize
            return when {
                size + 2 * room >= containerSize -> before
                before < 0 -> before
                after > 0 -> after
                else -> 0f
            }
        }
    }
}

