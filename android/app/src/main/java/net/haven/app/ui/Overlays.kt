package net.haven.app.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.clickable
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.scale
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import net.haven.app.R
import net.haven.app.app.AppModel
import net.haven.app.app.WhatsNew
import kotlin.math.PI
import kotlin.math.sin

/** The loading screen, over everything while Haven starts: the halo turning
 *  slowly with a pulse of gold light behind it, "Haven", and a bar with
 *  "Loading… NN%" (then "Ready"). Nothing underneath can be touched. */
@Composable
fun LoadingScreen(progress: Double, modifier: Modifier = Modifier) {
    val palette = LocalPalette.current
    val time = rememberInfiniteTransition(label = "loading")
    val turn by time.animateFloat(0f, 360f, infiniteRepeatable(tween(12_000, easing = LinearEasing)), label = "turn")
    val phase by time.animateFloat(0f, 1f, infiniteRepeatable(tween(1_600, easing = LinearEasing), RepeatMode.Restart), label = "beat")
    val beat = ((sin(phase * 2 * PI) + 1) / 2).toFloat()
    val shown by animateFloatAsState(progress.coerceIn(0.0, 1.0).toFloat(), tween(160, easing = LinearEasing), label = "bar")
    Box(
        modifier
            .fillMaxSize()
            .background(palette.background)
            .clickable(remember { MutableInteractionSource() }, indication = null) {}
            .semantics { contentDescription = "Loading" },
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.scale(if (LocalTV.current) 1.6f else 1f)) {
            Box(
                Modifier.size(300.dp, 190.dp).drawBehind {
                    drawCircle(
                        Brush.radialGradient(
                            listOf(palette.accent.copy(alpha = 0.10f + 0.14f * beat), Color.Transparent),
                            center = Offset(size.width / 2, size.height / 2),
                            radius = 105.dp.toPx(),
                        ),
                        radius = 150.dp.toPx(),
                    )
                },
                contentAlignment = Alignment.Center,
            ) {
                Image(painterResource(R.drawable.halo), null, Modifier.size(190.dp).rotate(turn), alpha = 0.85f + 0.15f * beat)
            }
            Spacer(Modifier.height(16.dp))
            Text("Haven", style = Type.heading(30.sp, palette.text))
            Spacer(Modifier.height(16.dp))
            Box(Modifier.width(260.dp).height(6.dp).clip(RoundedCornerShape(50)).background(palette.border)) {
                Box(Modifier.fillMaxWidth(shown).height(6.dp).clip(RoundedCornerShape(50)).background(palette.accent))
            }
            Spacer(Modifier.height(10.dp))
            Text(AppModel.splashText(progress), style = Type.font(13.sp, color = palette.textDim))
        }
    }
}

/** "What's New": this version's notes after the loading screen, with
 *  "Don't show again" and Done. */
@Composable
fun WhatsNewScreen(notes: WhatsNew.Notes, done: (dontShowAgain: Boolean) -> Unit) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    var hide by remember { mutableStateOf(false) }
    val doneFocus = remember { FocusRequester() }
    LaunchedEffect(Unit) { if (tv) runCatching { doneFocus.requestFocus() } }
    Column(
        Modifier
            .fillMaxSize()
            .background(palette.background)
            .safeDrawingPadding()
            .padding(top = 20.dp, bottom = 16.dp, start = 16.dp, end = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text("What's New", style = Type.heading(40.sp, palette.text))
            Text("Haven ${notes.version}", style = Type.font(14.sp, color = palette.textDim))
        }
        LazyColumn(
            Modifier.weight(1f).widthIn(max = 560.dp).fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            items(notes.sections) { section -> NotesCard(section) }
        }
        Row(
            Modifier
                .heightIn(min = 40.dp)
                .pressable(RoundedCornerShape(8.dp), role = Role.Checkbox) { hide = !hide }
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .size(24.dp)
                    .background(if (hide) palette.accent else Color.Transparent, RoundedCornerShape(6.dp))
                    .border(2.dp, if (hide) palette.accent else palette.textDim, RoundedCornerShape(6.dp)),
                contentAlignment = Alignment.Center,
            ) {
                if (hide) HavenIcon("check", 18.dp, palette.onAccent)
            }
            Spacer(Modifier.width(10.dp))
            Text("Don't show again", style = Type.font(16.sp, FontWeight.SemiBold, palette.text))
        }
        PrimaryButton("Done", Modifier.width(220.dp).focusRequester(doneFocus)) { done(hide) }
    }
}

@Composable
private fun NotesCard(section: WhatsNew.Notes.Section) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(14.dp)
    Column(Modifier.fillMaxWidth().clip(shape).background(palette.surface).border(1.dp, palette.border, shape)) {
        Box(Modifier.fillMaxWidth().heightIn(min = 40.dp).background(palette.accent).padding(horizontal = 14.dp), contentAlignment = Alignment.CenterStart) {
            Text(section.title, style = Type.heading(18.sp, palette.onAccent))
        }
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            section.lines.forEach { line ->
                Row {
                    Box(Modifier.padding(top = 6.dp).size(7.dp).background(palette.accent, RoundedCornerShape(2.dp)))
                    Spacer(Modifier.width(10.dp))
                    Text(line, style = Type.font(14.sp, color = palette.text))
                }
            }
        }
    }
}

/** Haven could not open its data (a database from a newer Haven, say). */
@Composable
fun CouldNotStart(reason: String) {
    val palette = LocalPalette.current
    Column(
        Modifier.fillMaxSize().background(palette.background).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Image(painterResource(R.drawable.halo), null, Modifier.size(120.dp))
        Text("Haven could not start", style = Type.heading(24.sp, palette.text))
        Text(reason, style = Type.font(14.sp, color = palette.textDim))
    }
}
