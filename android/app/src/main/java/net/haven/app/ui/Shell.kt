package net.haven.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import net.haven.app.R
import net.haven.app.app.AppModel
import net.haven.app.app.Layout
import net.haven.app.app.Page

/** Haven's window: the page showing, the tab bar along the bottom (on a
 *  tablet, the sidebar down the left; on a TV, the tabs along the top), and
 *  over them the loading screen and What's New. A tap outside a text field
 *  closes the keyboard. */
@Composable
fun Shell(model: AppModel) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    val focus = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    BoxWithConstraints(
        Modifier
            .fillMaxSize()
            .background(palette.background)
            .pointerInput(Unit) {
                detectTapGestures {
                    focus.clearFocus()
                    keyboard?.hide()
                }
            },
    ) {
        val layout = Layout.of(tv, maxWidth.value)
        val selectOrNull: (Page) -> Unit = { model.select(it) }
        when (layout) {
            Layout.Phone -> Column(Modifier.fillMaxSize()) {
                Box(Modifier.weight(1f).fillMaxWidth().statusBarsPadding()) { PageView(model) }
                TabBar(model.page, model.detailOpen, selectOrNull)
            }
            Layout.Tablet -> Row(Modifier.fillMaxSize()) {
                SideBar(model.page, model.detailOpen, selectOrNull)
                Box(Modifier.weight(1f).fillMaxHeight().safeDrawingPadding()) { PageView(model) }
            }
            Layout.TV -> Column(Modifier.fillMaxSize().padding(horizontal = 48.dp, vertical = 27.dp)) {
                TVTabs(model.page, model.detailOpen, enabled = model.splashGone && !model.whatsNewShown, selectOrNull)
                Box(Modifier.weight(1f).fillMaxWidth()) { PageView(model) }
            }
        }

        // A title's page (K12) lies over the pages.
        model.opening?.let { TitleStandIn(it.title) { model.closeTitle() } }

        // Back steps back: a title's page closes, a page other than Home goes
        // to Home, then Haven closes (Android's own Back).
        BackHandler(enabled = model.splashGone && !model.whatsNewShown && (model.page != Page.Home || model.opening != null)) {
            if (model.opening != null) model.closeTitle() else model.select(Page.Home)
        }

        if (!model.splashGone) {
            val alpha by animateFloatAsState(if (model.splashShown) 1f else 0f, tween(AppModel.SPLASH_FADE_MILLIS.toInt()), label = "splash")
            LoadingScreen(model.splashProgress, Modifier.alpha(alpha))
        }
        val notes = model.notes
        AnimatedVisibility(model.whatsNewShown && notes != null, enter = fadeIn(tween(200)), exit = fadeOut(tween(200))) {
            if (notes != null) {
                BackHandler { model.whatsNewDone(dontShowAgain = false) }
                WhatsNewScreen(notes) { model.whatsNewDone(it) }
            }
        }
    }
}

/** The page showing. Each page arrives in its own section (K11 Explore,
 *  K14 Library, K15 Live TV, K16 Calendar, K17 Settings). */
@Composable
private fun PageView(model: AppModel) {
    if (model.page == Page.Home) return HomePage(model)
    if (model.page == Page.Explore) return ExplorePage(model)
    val palette = LocalPalette.current
    val tv = LocalTV.current
    Column(Modifier.fillMaxSize().padding(horizontal = if (tv) 0.dp else 16.dp, vertical = 12.dp)) {
        PageTitle(model.page.label)
        Spacer(Modifier.height(8.dp))
        Text(
            "Haven ${net.haven.app.BuildConfig.VERSION_NAME} build ${net.haven.app.BuildConfig.HAVEN_BUILD}",
            style = Type.font(if (tv) 20.sp else 14.sp, color = palette.textDim),
        )
        if (model.page == Page.Settings) {
            Spacer(Modifier.height(16.dp))
            PrimaryButton(if (model.dark) "Light theme" else "Dark theme") { model.setDark(!model.dark) }
        }
    }
}

/** Haven's pages along the bottom on a phone: six buttons on a floating
 *  pill (icon above a small bold label), as the other builds have it. */
@Composable
fun TabBar(page: Page, detailOpen: Boolean, select: (Page) -> Unit) {
    val palette = LocalPalette.current
    val pill = RoundedCornerShape(33.dp)
    Box(Modifier.fillMaxWidth().navigationBarsPadding().padding(start = 12.dp, end = 12.dp, top = 6.dp, bottom = 6.dp)) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(66.dp)
                .shadow(11.dp, pill, ambientColor = Color.Black.copy(alpha = if (palette.dark) 0.5f else 0.15f), spotColor = Color.Black.copy(alpha = if (palette.dark) 0.5f else 0.15f))
                .background(palette.surface, pill)
                .border(1.dp, palette.border, pill)
                .padding(5.dp),
        ) {
            Page.entries.forEach { tab ->
                val selected = !detailOpen && page == tab
                val shape = RoundedCornerShape(50)
                Column(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .background(if (selected) palette.selected else Color.Transparent, shape)
                        .pressable(shape, role = Role.Tab) { select(tab) }
                        .semantics { this.selected = selected },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    val tint = if (selected) palette.text else palette.textDim
                    HavenIcon(tab.icon, 22.dp, tint)
                    Spacer(Modifier.height(3.dp))
                    Text(tab.label, style = Type.font(11.sp, FontWeight.Bold, tint), maxLines = 1, overflow = TextOverflow.Clip)
                }
            }
        }
    }
}

/** Haven's pages down the left on a tablet: the halo, then a row for each
 *  page (icon and name), the chosen one filled. */
@Composable
fun SideBar(page: Page, detailOpen: Boolean, select: (Page) -> Unit) {
    val palette = LocalPalette.current
    Row(Modifier.fillMaxHeight()) {
        Column(
            Modifier
                .width(220.dp)
                .fillMaxHeight()
                .background(palette.surface)
                .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Vertical + WindowInsetsSides.Start))
                .padding(start = 12.dp, end = 12.dp, top = 20.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(Modifier.padding(start = 12.dp, end = 12.dp, bottom = 18.dp), verticalAlignment = Alignment.CenterVertically) {
                Image(painterResource(R.drawable.halo), null, Modifier.size(34.dp))
                Spacer(Modifier.width(10.dp))
                Text("Haven", style = Type.heading(24.sp, palette.text))
            }
            Page.entries.forEach { tab ->
                val selected = !detailOpen && page == tab
                val shape = RoundedCornerShape(12.dp)
                val tint = if (selected) palette.text else palette.textDim
                Row(
                    Modifier
                        .fillMaxWidth()
                        .height(48.dp)
                        .background(if (selected) palette.selected else Color.Transparent, shape)
                        .pressable(shape, role = Role.Tab) { select(tab) }
                        .semantics { this.selected = selected }
                        .padding(horizontal = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    HavenIcon(tab.icon, 22.dp, tint)
                    Spacer(Modifier.width(12.dp))
                    Text(tab.label, style = Type.font(16.sp, FontWeight.Bold, tint), maxLines = 1)
                }
            }
        }
        Box(Modifier.width(1.dp).fillMaxHeight().background(palette.border))
    }
}

/** Haven's pages along the top on a TV, as Apple TV's tab bar: moved through
 *  with the remote's left and right; the page shows as the remote lands on
 *  its tab. The remote starts on the page that was showing. */
@Composable
fun TVTabs(page: Page, detailOpen: Boolean, enabled: Boolean, select: (Page) -> Unit) {
    val palette = LocalPalette.current
    val start = remember { FocusRequester() }
    LaunchedEffect(enabled) { if (enabled) runCatching { start.requestFocus() } }
    Row(
        Modifier.fillMaxWidth().padding(bottom = 24.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Six tabs on one line, whatever the TV's size (they wrapped before).
        Page.entries.forEach { tab ->
            val selected = !detailOpen && page == tab
            val shape = RoundedCornerShape(50)
            Box(
                Modifier
                    .then(if (tab == page) Modifier.focusRequester(start) else Modifier)
                    .background(if (selected) palette.selected else Color.Transparent, shape)
                    .onFocusSelect(enabled) { if (page != tab) select(tab) }
                    .pressable(shape, role = Role.Tab, enabled = enabled) { select(tab) }
                    .semantics { this.selected = selected }
                    .padding(horizontal = 20.dp, vertical = 10.dp),
            ) {
                Text(tab.label, style = Type.font(20.sp, FontWeight.Bold, if (selected) palette.text else palette.textDim), maxLines = 1, softWrap = false)
            }
        }
    }
}

/** Calls `select` when the remote lands on this. */
private fun Modifier.onFocusSelect(enabled: Boolean, select: () -> Unit): Modifier =
    if (!enabled) this else this.onFocusChanged { if (it.isFocused) select() }

/** Stands in for a title's page until K12: its name, and Close. */
@Composable
private fun TitleStandIn(title: String, close: () -> Unit) {
    val palette = LocalPalette.current
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    Column(
        Modifier.fillMaxSize().background(palette.background).safeDrawingPadding().padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = Type.heading(28.sp, palette.text))
        Text("The title's page comes in the next section.", style = Type.font(14.sp, color = palette.textDim))
        PrimaryButton("Close", Modifier.focusRequester(focus), onClick = close)
    }
}
