package net.haven.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** A page's title ("Home", "Library"…). */
@Composable
fun PageTitle(text: String, modifier: Modifier = Modifier) {
    val tv = LocalTV.current
    Text(text, modifier, style = Type.heading(if (tv) 40.sp else 26.sp, LocalPalette.current.text))
}

/** Something that can be pressed: by touch it darkens a little while held;
 *  on a TV it shows a gold ring while the remote is on it. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun Modifier.pressable(shape: Shape, role: Role = Role.Button, enabled: Boolean = true, onLongClick: (() -> Unit)? = null, onClick: () -> Unit): Modifier {
    val source = remember { MutableInteractionSource() }
    val pressed by source.collectIsPressedAsState()
    val focused by source.collectIsFocusedAsState()
    val palette = LocalPalette.current
    return this
        .border(if (focused) 3.dp else 0.dp, if (focused) palette.accent else Color.Transparent, shape)
        .combinedClickable(source, indication = null, enabled = enabled, role = role, onLongClick = onLongClick, onClick = onClick)
        .alpha(if (pressed) 0.75f else 1f)
}

/** The gold button. */
@Composable
fun PrimaryButton(text: String, modifier: Modifier = Modifier, enabled: Boolean = true, onClick: () -> Unit) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(50)
    Box(
        modifier
            .defaultMinSize(minHeight = if (LocalTV.current) 52.dp else 34.dp)
            .background(if (enabled) palette.accent else palette.selected, shape)
            .pressable(shape, enabled = enabled, onClick = onClick)
            .padding(horizontal = 18.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, style = Type.font(if (LocalTV.current) 20.sp else 13.sp, FontWeight.Bold, if (enabled) palette.onAccent else palette.textDim))
    }
}
