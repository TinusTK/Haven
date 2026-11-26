package net.haven.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Fill
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** One of Haven's own icons, drawn from [IconPaths] (a 2 dp line; "play"
 *  is filled), so they look the same as in the other builds. */
@Composable
fun HavenIcon(name: String, size: Dp = 24.dp, tint: Color = LocalPalette.current.text, stroke: Dp = 2.dp, modifier: Modifier = Modifier) {
    val path: Path? = remember(name) {
        IconPaths.drawings[name]?.let { PathParser().parsePathString(it).toPath() }
    }
    Canvas(modifier.size(size)) {
        path ?: return@Canvas
        val s = minOf(this.size.width, this.size.height) / 24f
        scale(s, pivot = Offset.Zero) {
            drawPath(path, tint, style = if (name == "play") Fill else Stroke(width = stroke.toPx() / s))
        }
    }
}
