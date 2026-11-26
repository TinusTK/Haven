package net.haven.app.ui

import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.delay
import net.haven.app.player.MpvEngine
import net.haven.app.player.PlayerEngine
import net.haven.app.player.PlayerModel
import net.haven.app.player.PlayerModel.Menu
import net.haven.app.player.PlayerRules
import net.haven.app.player.Resume

private val panel = Color(0xF5151821)

/** The player page: mpv's picture, and over it the controls. A tap (or any
 *  key on a remote) shows them; they hide by themselves after 3 s unless
 *  something keeps them up (paused, loading, a list open, an error, the next
 *  episode). Back closes a list first, then the page. In picture in picture
 *  only the picture shows. */
@Composable
fun PlayerView(player: PlayerModel, pictureInPicture: Boolean) {
    val tv = LocalTV.current
    val pinned = player.paused || player.busy || player.menu != Menu.None || player.error.isNotEmpty() || player.nextCountdown != null
    var touched by remember { mutableLongStateOf(0L) }
    val keys = remember { FocusRequester() }
    val view = LocalView.current
    // The screen stays on while the player shows.
    androidx.compose.runtime.DisposableEffect(Unit) {
        view.keepScreenOn = true
        onDispose { view.keepScreenOn = false }
    }
    player.screenHeight = LocalConfiguration.current.screenHeightDp.toDouble()
    LaunchedEffect(player.controlsShown, pinned, touched) {
        player.placeSubtitles()
        if (!player.controlsShown || pinned) return@LaunchedEffect
        delay(PlayerRules.HIDE_AFTER_MILLIS)
        player.controlsShown = false
    }
    LaunchedEffect(player.controlsShown) { if (!player.controlsShown) runCatching { keys.requestFocus() } }
    BackHandler {
        when {
            player.menu != Menu.None -> player.menu = Menu.None
            tv && player.controlsShown && !pinned -> player.controlsShown = false
            else -> player.close()
        }
    }
    Box(
        Modifier.fillMaxSize().background(Color.Black)
            .focusRequester(keys)
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                touched = System.currentTimeMillis()
                if (player.controlsShown || player.menu != Menu.None) return@onPreviewKeyEvent false
                // Controls hidden: the remote moves through the film.
                when (event.key) {
                    Key.DirectionLeft -> player.skip(-10.0)
                    Key.DirectionRight -> player.skip(30.0)
                    Key.MediaPlayPause, Key.DirectionCenter, Key.Enter -> player.togglePause()
                    else -> player.controlsShown = true
                }
                if (event.key == Key.DirectionCenter || event.key == Key.Enter) player.controlsShown = true
                true
            }
            .focusable(),
    ) {
        MpvSurface(player)
        if (pictureInPicture) return@Box
        // The picture: a tap shows the controls, or hides them.
        Box(Modifier.fillMaxSize().pointerInput(Unit) {
            detectTapGestures {
                if (player.menu != Menu.None) player.menu = Menu.None else player.controlsShown = !player.controlsShown
                touched = System.currentTimeMillis()
            }
        })
        if (player.busy && player.error.isEmpty()) {
            Text("Loading…", Modifier.align(Alignment.Center).offset(y = 80.dp), style = Type.font(16.sp, color = Color.White))
        }
        if (player.error.isNotEmpty()) {
            Text(
                player.error,
                Modifier.align(Alignment.Center).padding(horizontal = 32.dp).widthIn(max = 520.dp).background(panel, RoundedCornerShape(12.dp)).padding(24.dp),
                style = Type.font(15.sp, color = Color.White),
            )
        }
        AnimatedVisibility(player.controlsShown || pinned, enter = fadeIn(), exit = fadeOut()) {
            Controls(player, onTouch = { touched = System.currentTimeMillis() })
        }
        if (player.menu != Menu.None) MenuPanel(player, Modifier.align(Alignment.BottomEnd))
        player.nextCountdown?.let { NextCard(player, it, Modifier.align(Alignment.BottomEnd)) }
    }
}

/** mpv's Surface, behind everything. */
@Composable
private fun MpvSurface(player: PlayerModel) {
    val engine = player.engine as? MpvEngine ?: return
    AndroidView({ context ->
        SurfaceView(context).apply {
            holder.addCallback(object : SurfaceHolder.Callback {
                override fun surfaceCreated(holder: SurfaceHolder) = engine.surfaceCreated(holder.surface)
                override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) = engine.surfaceChanged(width, height)
                override fun surfaceDestroyed(holder: SurfaceHolder) = engine.surfaceDestroyed()
            })
        }
    }, Modifier.fillMaxSize())
}

@Composable
private fun Controls(player: PlayerModel, onTouch: () -> Unit) {
    val tv = LocalTV.current
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { if (tv) runCatching { first.requestFocus() } }
    Column(Modifier.fillMaxSize()) {
        // Top: back, what is playing, how the stream comes in, when it ends.
        Row(
            Modifier.fillMaxWidth().background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.75f), Color.Transparent)))
                .safeDrawingPadding().padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            PlayerButton("‹ Back", name = "Back") { player.close() }
            Column(Modifier.weight(1f)) {
                Text(player.title, style = Type.font(17.sp, FontWeight.SemiBold, Color.White), maxLines = 1)
                if (player.subtitle.isNotEmpty()) Text(player.subtitle, style = Type.font(13.sp, color = Color.White.copy(alpha = 0.7f)), maxLines = 1)
            }
            Column(horizontalAlignment = Alignment.End) {
                if (player.net.isNotEmpty()) Text(player.net, style = Type.font(13.sp, FontWeight.SemiBold, Color.White))
                if (player.endsAt.isNotEmpty()) Text(player.endsAt, style = Type.font(12.sp, color = Color.White.copy(alpha = 0.7f)))
            }
        }
        Spacer(Modifier.weight(1f))
        // Middle: back 10 seconds, play or pause, forward 30 seconds.
        if (player.error.isEmpty()) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(40.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
                if (player.duration > 0) RoundButton("back", "10", 64.dp, "Back 10 seconds") { player.skip(-10.0); onTouch() }
                RoundButton(if (player.paused) "play" else "pause", null, 84.dp, if (player.paused) "Play" else "Pause", Modifier.focusRequester(first)) {
                    player.togglePause()
                    onTouch()
                }
                if (player.duration > 0) RoundButton("forward", "30", 64.dp, "Forward 30 seconds") { player.skip(30.0); onTouch() }
            }
        }
        Spacer(Modifier.weight(1f))
        // Bottom: the position and the buttons.
        if (player.error.isEmpty()) {
            Column(
                Modifier.fillMaxWidth().background(Brush.verticalGradient(listOf(Color.Transparent, Color.Black.copy(alpha = 0.85f))))
                    .safeDrawingPadding().padding(start = 20.dp, end = 20.dp, top = 24.dp, bottom = 12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(player.elapsedText, Modifier.widthIn(min = 52.dp), style = Type.font(13.sp, FontWeight.SemiBold, Color.White))
                    SeekBar(player.duration, player.position, player.buffered, Modifier.weight(1f)) { player.seekTo(it); onTouch() }
                    Text(player.remainingText, Modifier.widthIn(min = 52.dp), style = Type.font(13.sp, color = Color.White.copy(alpha = 0.7f)))
                }
                LazyRow(Modifier.tvRow(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    val prefs = player.preferences
                    if (tv) {
                        item {
                            PlayerButton(if (prefs.volume > 1.001) "Sound ${Math.round(prefs.volume * 100)}%" else "Sound", active = player.menu == Menu.Sound || prefs.volume > 1.001) {
                                player.toggleMenu(Menu.Sound)
                            }
                        }
                    } else {
                        item {
                            val silent = prefs.muted || prefs.volume <= 0
                            PlayerButton(if (silent) "Muted" else "Volume", name = if (prefs.muted) "Turn the sound on" else "Mute", active = silent) { player.toggleMute() }
                        }
                        item {
                            Slider(
                                if (prefs.muted) 0f else prefs.volume.toFloat(), { player.setVolume(it.toDouble()); onTouch() },
                                Modifier.width(if (prefs.loud) 150.dp else 110.dp).semantics { contentDescription = "Volume" },
                                valueRange = 0f..prefs.top(false).toFloat(),
                                colors = SliderDefaults.colors(thumbColor = LocalPalette.current.accent, activeTrackColor = LocalPalette.current.accent),
                            )
                        }
                        // Above 100%, how loud, in gold.
                        if (prefs.loud && !prefs.muted && prefs.volume > 1.001) item {
                            Text("${Math.round(prefs.volume * 100)}%", style = Type.font(13.sp, FontWeight.Bold, LocalPalette.current.accent))
                        }
                    }
                    if (player.audioTracks.size > 1) item { PlayerButton("Audio", active = player.menu == Menu.Audio) { player.toggleMenu(Menu.Audio) } }
                    item { PlayerButton("Subtitles", active = player.menu == Menu.Subtitles) { player.toggleMenu(Menu.Subtitles) } }
                    item {
                        PlayerButton(if (player.speed == 1.0) "Speed" else "${PlayerRules.formatted(player.speed)}×", name = "Speed", active = player.menu == Menu.Speed || player.speed != 1.0) {
                            player.toggleMenu(Menu.Speed)
                        }
                    }
                    item { PlayerButton("Picture", active = player.menu == Menu.Picture || player.fit != PlayerEngine.Fit.Whole) { player.toggleMenu(Menu.Picture) } }
                    item { PlayerButton("Info", active = player.menu == Menu.Info) { player.toggleMenu(Menu.Info) } }
                    if (player.nextTitle.isNotEmpty()) item { PlayerButton("Next ›", name = "Next episode: ${player.nextTitle}") { player.goNext() } }
                }
            }
        }
    }
}

/** A word button on the player's bars. */
@Composable
private fun PlayerButton(label: String, modifier: Modifier = Modifier, name: String? = null, active: Boolean = false, onClick: () -> Unit) {
    val shape = RoundedCornerShape(50)
    Box(
        modifier.height(38.dp).background(Color.White.copy(alpha = if (active) 0.16f else 0.1f), shape).pressable(shape, onClick = onClick)
            .semantics { contentDescription = name ?: label }.padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Type.font(14.sp, FontWeight.SemiBold, if (active) LocalPalette.current.accent else Color.White), maxLines = 1)
    }
}

/** A round button in the middle of the picture. */
@Composable
private fun RoundButton(icon: String, caption: String?, size: Dp, name: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Column(
        modifier.size(size).background(Color.Black.copy(alpha = 0.45f), CircleShape).border(1.dp, Color.White.copy(alpha = 0.25f), CircleShape)
            .pressable(CircleShape, onClick = onClick).semantics { contentDescription = name },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        HavenIcon(icon, size * 0.38f, Color.White, stroke = 2.4.dp)
        if (caption != null) Text(caption, style = Type.font(11.sp, FontWeight.Bold, Color.White))
    }
}

/** The position: what has arrived in a lighter shade, a gold part for what
 *  was played, and a bubble with the time while dragging. */
@Composable
private fun SeekBar(duration: Double, position: Double, buffered: Double, modifier: Modifier, seek: (Double) -> Unit) {
    val accent = LocalPalette.current.accent
    var dragging by remember { mutableStateOf<Double?>(null) }
    BoxWithConstraints(
        modifier.height(30.dp)
            .semantics { contentDescription = "Position ${Resume.clock(position)} of ${Resume.clock(duration)}" }
            .pointerInput(duration) {
                detectTapGestures { offset -> if (duration > 0) seek((offset.x / size.width).coerceIn(0f, 1f) * duration) }
            }
            .pointerInput(duration) {
                detectHorizontalDragGestures(
                    onDragStart = { if (duration > 0) dragging = (it.x / size.width).coerceIn(0f, 1f) * duration },
                    onDragEnd = { dragging?.let(seek); dragging = null },
                    onDragCancel = { dragging = null },
                ) { change, _ -> if (duration > 0) dragging = (change.position.x / size.width).coerceIn(0f, 1f) * duration }
            },
        contentAlignment = Alignment.CenterStart,
    ) {
        val shown = dragging ?: position
        val share = if (duration > 0) (shown / duration).coerceIn(0.0, 1.0).toFloat() else 0f
        val arrived = if (duration > 0) (buffered / duration).coerceIn(0.0, 1.0).toFloat() else 0f
        Box(Modifier.fillMaxWidth().height(4.dp).background(Color.White.copy(alpha = 0.22f), RoundedCornerShape(50)))
        Box(Modifier.fillMaxWidth(arrived).height(4.dp).background(Color.White.copy(alpha = 0.4f), RoundedCornerShape(50)))
        Box(Modifier.fillMaxWidth(share).height(4.dp).background(accent, RoundedCornerShape(50)))
        val knob = if (dragging == null) 14.dp else 20.dp
        Box(Modifier.offset(x = maxWidth * share - knob / 2).size(knob).background(accent, CircleShape))
        dragging?.let {
            Text(
                Resume.clock(it),
                Modifier.offset(x = (maxWidth * share - 28.dp).coerceIn(0.dp, maxOf(maxWidth - 56.dp, 0.dp)), y = (-28).dp)
                    .background(Color.White, RoundedCornerShape(50)).padding(horizontal = 8.dp, vertical = 3.dp),
                style = Type.font(12.sp, FontWeight.Bold, Color.Black),
            )
        }
    }
}

/** The open list: audio, subtitles, speed, picture, sound or info. */
@Composable
private fun MenuPanel(player: PlayerModel, modifier: Modifier) {
    val heading = when (player.menu) {
        Menu.Audio -> "Audio"
        Menu.Subtitles -> "Subtitles"
        Menu.Speed -> "Speed"
        Menu.Picture -> "Picture"
        Menu.Sound -> "Sound boost"
        else -> "About what is playing"
    }
    val first = remember { FocusRequester() }
    LaunchedEffect(player.menu) { runCatching { first.requestFocus() } }
    Column(
        modifier.padding(start = 16.dp, end = 16.dp, bottom = 130.dp).widthIn(max = 360.dp).heightIn(max = 380.dp)
            .background(panel, RoundedCornerShape(12.dp)).border(1.dp, Color.White.copy(alpha = 0.15f), RoundedCornerShape(12.dp)).padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(heading, Modifier.padding(start = 8.dp, top = 4.dp), style = Type.font(12.sp, FontWeight.SemiBold, Color.White.copy(alpha = 0.7f)))
        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            var focusGiven = false
            fun focus(): Modifier = if (focusGiven) Modifier else Modifier.focusRequester(first).also { focusGiven = true }
            when (player.menu) {
                Menu.Audio -> player.audioTracks.forEach { Choice(it.label, it.selected, focus()) { player.chooseAudio(it.id) } }
                Menu.Subtitles -> {
                    Choice("Off", !player.anySubtitle, focus()) { player.chooseSubtitle(null) }
                    player.subtitleTracks.forEach { Choice(it.label, it.selected) { player.chooseSubtitle(it.id) } }
                    if (player.subtitleTracks.isEmpty()) Text("This has no subtitles of its own.", Modifier.padding(10.dp), style = Type.font(13.sp, color = Color.White.copy(alpha = 0.7f)))
                    Step("Size", "${Math.round(player.preferences.subtitleScale * 100)} %") { player.stepSubtitleScale(it) }
                    Step("Shown later by", "${PlayerRules.formatted(Math.rint(player.subtitleDelay * 10) / 10)} s") { player.stepSubtitleDelay(it) }
                    if (player.subtitleTracks.isNotEmpty()) {
                        Text("Out of step? As someone starts to speak:", Modifier.padding(start = 10.dp, end = 10.dp, top = 6.dp), style = Type.font(12.sp, color = Color.White.copy(alpha = 0.7f)))
                        Row(Modifier.padding(horizontal = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            PlayerButton("Next line now") { player.syncSubtitles(1) }
                            PlayerButton("Line before") { player.syncSubtitles(-1) }
                            PlayerButton("Reset") { player.syncSubtitles(0) }
                        }
                    }
                }
                Menu.Speed -> PlayerRules.speeds.forEach { speed -> Choice(PlayerRules.speedLabel(speed), player.speed == speed, focus()) { player.chooseSpeed(speed) } }
                Menu.Picture -> PlayerEngine.Fit.entries.forEach { fit -> Choice(PlayerRules.fits[fit.ordinal], player.fit == fit, focus()) { player.chooseFit(fit) } }
                Menu.Sound -> PlayerRules.Preferences.boosts.forEach { boost ->
                    Choice(if (boost == 1.0) "As recorded (100%)" else "${Math.round(boost * 100)}%", Math.abs(player.preferences.volume - boost) < 0.001, focus()) {
                        player.setVolume(boost)
                        player.menu = Menu.None
                    }
                }
                else -> Text(player.info.ifEmpty { "Nothing is known yet." }, Modifier.padding(10.dp).then(focus()).focusable(), style = Type.font(13.sp, color = Color.White))
            }
        }
    }
}

@Composable
private fun Choice(label: String, selected: Boolean, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val shape = RoundedCornerShape(8.dp)
    Row(
        modifier.fillMaxWidth().heightIn(min = 40.dp).background(if (selected) Color.White.copy(alpha = 0.08f) else Color.Transparent, shape)
            .pressable(shape, onClick = onClick).padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, Modifier.weight(1f), style = Type.font(14.sp, color = Color.White))
        if (selected) HavenIcon("check", 16.dp, LocalPalette.current.accent)
    }
}

@Composable
private fun Step(label: String, value: String, step: (Int) -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 44.dp).padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, Modifier.weight(1f), style = Type.font(14.sp, color = Color.White))
        PlayerButton("−", name = "$label: less") { step(-1) }
        Text(value, Modifier.widthIn(min = 56.dp), style = Type.font(14.sp, FontWeight.SemiBold, Color.White))
        PlayerButton("+", name = "$label: more") { step(1) }
    }
}

/** The next episode is about to start by itself. */
@Composable
private fun NextCard(player: PlayerModel, countdown: Int, modifier: Modifier) {
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    Column(
        modifier.padding(start = 16.dp, end = 16.dp, bottom = 130.dp).widthIn(max = 380.dp)
            .background(panel, RoundedCornerShape(12.dp)).border(1.dp, Color.White.copy(alpha = 0.15f), RoundedCornerShape(12.dp)).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text("Next episode in $countdown s", style = Type.font(13.sp, FontWeight.SemiBold, LocalPalette.current.accent))
        Text(player.nextTitle, style = Type.font(16.sp, FontWeight.SemiBold, Color.White), maxLines = 2)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PlayerButton("▶  Play now", Modifier.focusRequester(first)) { player.goNext() }
            PlayerButton("Cancel") { player.cancelNext() }
        }
    }
}

/** What plays on a Chromecast: its name, the time, and pause, back 10,
 *  forward 30 and stop. Sits at the bottom of every page while casting. */
@Composable
fun CastBar(cast: net.haven.app.cast.CastModel) {
    val palette = LocalPalette.current
    if (!cast.casting) return
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp).background(palette.surface, RoundedCornerShape(16.dp))
            .border(1.dp, palette.border, RoundedCornerShape(16.dp)).padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(cast.title, style = Type.font(14.sp, FontWeight.SemiBold, palette.text), maxLines = 1)
            Text(cast.status.ifEmpty { "On ${cast.deviceName}  ·  ${cast.time}" }, style = Type.font(12.sp, color = if (cast.status.startsWith("Casting stopped")) palette.alert else palette.textDim), maxLines = 1)
        }
        Chip("‹ 10", false) { cast.command(net.haven.app.cast.CastCommand.Back) }
        Chip(if (cast.paused) "▶" else "❙❙", false) { cast.togglePause() }
        Chip("30 ›", false) { cast.command(net.haven.app.cast.CastCommand.Forward) }
        Chip("Stop", false) { cast.command(net.haven.app.cast.CastCommand.Stop) }
    }
}
