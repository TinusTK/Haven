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
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
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
    val tv = LocalTV.current
    val layer = LocalFocusLayer.current
    val requester = remember { FocusRequester() }
    return this
        .border(if (focused) 3.dp else 0.dp, if (focused) palette.accent else Color.Transparent, shape)
        // On a TV the last thing the remote was on is kept, to go back to it.
        .then(
            if (!tv) Modifier else Modifier.focusRequester(requester).onFocusChanged {
                if (it.isFocused) FocusMemory.keep(layer, requester)
            },
        )
        // Off, it still takes the remote's focus and only ignores presses: a
        // button that went grey under the remote dropped the focus, and the
        // next arrow press then landed on the tabs (or nowhere).
        .combinedClickable(
            source, indication = null, role = role,
            onLongClick = onLongClick?.let { long -> { if (enabled) long() } },
            onClick = { if (enabled) onClick() },
        )
        .then(if (enabled) Modifier else Modifier.semantics { disabled() })
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

/** Where a pressable thing lies: on a page, or on a title's page over it. */
enum class FocusLayer { Page, Title, Other }

val LocalFocusLayer = staticCompositionLocalOf { FocusLayer.Other }

/** On a TV: the last thing the remote was on, on the pages and on a title's
 *  page, so closing a title (or the player) puts the remote back there. */
object FocusMemory {
    var page: FocusRequester? = null
        private set
    var title: FocusRequester? = null
        private set

    fun keep(layer: FocusLayer, requester: FocusRequester) {
        when (layer) {
            FocusLayer.Page -> page = requester
            FocusLayer.Title -> title = requester
            FocusLayer.Other -> Unit
        }
    }

    /** Puts the remote back on `layer`'s last thing; false if it is gone
     *  (scrolled away, or the page changed). */
    fun restore(layer: FocusLayer): Boolean {
        val requester = (if (layer == FocusLayer.Title) title else page) ?: return false
        return runCatching { requester.requestFocus() }.isSuccess
    }

    fun forgetTitle() {
        title = null
    }
}
