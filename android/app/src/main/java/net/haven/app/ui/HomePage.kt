package net.haven.app.ui

import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import net.haven.app.BuildConfig
import net.haven.app.app.AppModel
import net.haven.app.app.Page
import net.haven.app.library.HomeTop
import net.haven.app.library.Library
import net.haven.app.library.Opening
import net.haven.app.library.Shelf
import net.haven.app.library.Releases
import net.haven.app.pictures.Pause
import net.haven.app.pictures.PictureSize
import net.haven.app.titles.MetaPreview
import java.time.Instant

/** Home: the banner, Watch now, Upcoming, Continue Watching, the Watchlist,
 *  then the shelves (your media servers first, then Cinemeta and add-ons,
 *  then TMDB). Pull down to load it afresh. */
@Composable
fun HomePage(model: AppModel) {
    val library = model.library
    DisposableEffect(Unit) {
        model.homeAppeared(true)
        onDispose { model.homeAppeared(false) }
    }
    if (library.homeHasSources) HomeFilled(model, library) else HomeEmpty(model)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HomeFilled(model: AppModel, library: Library) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    val scope = rememberCoroutineScope()
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val gutter = if (tv) 0.dp else 16.dp
        val width = maxWidth - gutter * 2
        val wide = if (tv) PosterFit.TV_WIDE.dp else minOf(maxOf((width - 14.dp) / 2, 0.dp), 380.dp)
        PullToRefreshBox(isRefreshing = library.refreshing, onRefresh = { scope.launch { library.refreshHome() } }) {
            LazyColumn(
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = gutter, end = gutter, top = 12.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(28.dp),
            ) {
                if (library.hero.isNotEmpty()) item("hero") {
                    HeroBanner(library.hero, width, showing = model.page == Page.Home && !model.detailOpen, pause = library.homePause,
                        firstPicture = { library.heroFirstPicture = true }) { slide ->
                        model.open(Opening(slide.id, slide.kind, slide.title))
                    }
                }
                if (library.watchNow.isNotEmpty()) item("watchNow") {
                    ReleaseRow("Watch now", library.watchNow, wide, library.homePause, { model.openLibrary(2) }) { model.open(it) }
                }
                if (library.upcoming.isNotEmpty()) item("upcoming") {
                    ReleaseRow("Upcoming", library.upcoming, wide, library.homePause, { model.select(Page.Calendar) }) { model.open(it) }
                }
                if (library.continueCards.isNotEmpty()) item("continue") {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        RowHeading("Continue Watching") { model.openLibrary(3) }
                        LazyRow(Modifier.tvRow(), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                            items(library.continueCards, key = { it.kind + it.id }) { card -> ContinueItem(card, wide, library, model) }
                        }
                    }
                }
                if (library.watchlist.isNotEmpty()) item("watchlist") {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        RowHeading("Watchlist") { model.openLibrary(2) }
                        PosterRow(library.watchlist.map { MetaPreview(it.titleId, it.kind, it.title, it.poster, it.year) }, width, model, library.homePause)
                    }
                }
                if (library.homeBoard.shelves.isEmpty() && library.serverBoard.shelves.isEmpty() && library.catalogBoard.shelves.isEmpty()) item("note") {
                    Text(
                        when {
                            library.servers.enabled.isNotEmpty() -> "Waiting for your media servers…"
                            BuildConfig.ADDON_SCREENS -> "Your add-ons have no catalogues to show here."
                            else -> "Nothing to show here yet."
                        },
                        style = Type.font(14.sp, color = palette.textDim),
                    )
                }
                // Your own libraries first, then catalogues, then TMDB.
                items(library.serverBoard.shelves.filter { !(it.items.isEmpty() && !it.loading && it.error.isEmpty()) }, key = { "s${it.id}|${it.title}" }) {
                    ShelfRow(it, width, model)
                }
                items(library.homeBoard.shelves, key = { "h${it.id}|${it.title}" }) { ShelfRow(it, width, model) }
                items(library.catalogBoard.shelves, key = { "c${it.id}|${it.title}" }) { ShelfRow(it, width, model) }
            }
        }
    }
}

@Composable
private fun HomeEmpty(model: AppModel) {
    val palette = LocalPalette.current
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(32.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        PageTitle("Home")
        Text(net.haven.app.library.Library.emptyText(BuildConfig.ADDON_SCREENS), style = Type.font(14.sp, color = palette.textDim))
        PrimaryButton("Open Settings") { model.select(Page.Settings) }
    }
}

@Composable
private fun ReleaseRow(title: String, cards: List<Releases.Card>, wide: Dp, pause: Pause, heading: () -> Unit, open: (Opening) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        RowHeading(title, heading)
        // Counts down by the hour on the day.
        val now by produceState(Instant.now()) {
            while (true) {
                delay(30_000)
                value = Instant.now()
            }
        }
        LazyRow(Modifier.tvRow(), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            items(cards, key = { it.kind + it.id }) { card ->
                WideCard(
                    card.title, card.episode, card.picture, card.badge, wide,
                    Modifier.pressable(RoundedCornerShape(16.dp)) { open(Opening(card.id, card.kind, card.title, card.video.ifEmpty { null })) },
                    pause = pause,
                ) {
                    WhenPill(card.whenAt(now), card.out || card.soon, Modifier.align(Alignment.BottomEnd))
                }
            }
        }
    }
}

@Composable
private fun ContinueItem(card: HomeTop.ContinueCard, wide: Dp, library: Library, model: AppModel) {
    var menu by remember { mutableStateOf(false) }
    Box {
        WideCard(
            card.title, card.episode, card.picture, card.badge, wide,
            Modifier.pressable(RoundedCornerShape(16.dp), onLongClick = { menu = true }) {
                model.open(Opening(card.id, card.kind, card.title, card.video))
            },
            progress = card.progress, pause = library.homePause,
        ) {
            if (card.left > 0) LeftPill(card.left, Modifier.align(Alignment.TopEnd))
        }
        DropdownMenu(menu, { menu = false }) {
            DropdownMenuItem({ Text("Remove from Continue Watching", style = Type.font(15.sp, color = LocalPalette.current.alert)) }, {
                menu = false
                library.removeContinue(card.kind, card.id)
            })
        }
    }
}

/** The banner: a few titles of the first shelf in turn, large, every 9 s
 *  while Home shows, with dots for which one shows. */
@Composable
private fun HeroBanner(slides: List<HomeTop.Slide>, width: Dp, showing: Boolean, pause: Pause, firstPicture: () -> Unit, open: (HomeTop.Slide) -> Unit) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    var index by remember { mutableIntStateOf(0) }
    // On a TV about a third as high as it is wide (Apple TV's 620 of 1760
    // points), so the first row shows under it.
    val height = when {
        tv -> minOf(width * 0.36f, 310.dp)
        width.value > PosterFit.WIDE -> minOf(width * 0.45f, 480.dp)
        else -> minOf(maxOf(width * 1.2f, 420.dp), 560.dp)
    }
    LaunchedEffect(showing, slides.size) {
        if (!showing || slides.size < 2) return@LaunchedEffect
        while (true) {
            delay(HomeTop.TURN_EVERY_MILLIS)
            index = (index + 1) % slides.size
        }
    }
    val shape = RoundedCornerShape(24.dp)
    val shade = Color(0xFF0D0F14)
    Column(verticalArrangement = Arrangement.spacedBy(14.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        val slide = slides[minOf(index, slides.size - 1)]
        // The frame that takes the remote stays while the slides fade inside
        // it: were it part of a slide, the remote would lose its place (and
        // land on the tabs) each time the banner turned.
        Box(
            Modifier.size(width, height).clip(shape)
                // Left and right on the banner turn it by hand (on a TV).
                .onPreviewKeyEvent { event ->
                    val step = when (event.key) {
                        Key.DirectionRight -> 1
                        Key.DirectionLeft -> slides.size - 1
                        else -> 0
                    }
                    if (!tv || step == 0 || slides.size < 2) return@onPreviewKeyEvent false
                    if (event.type == KeyEventType.KeyDown) index = (index + step) % slides.size
                    true
                }
                .pressable(shape) { open(slide) }
                .semantics { contentDescription = "Info about ${slide.title}" },
        ) {
        AnimatedContent(slide, transitionSpec = { fadeIn(tween(300)) togetherWith fadeOut(tween(300)) }, label = "banner") { shown ->
            Box(
                Modifier.size(width, height).background(palette.surface).border(1.dp, palette.border, shape)
                    .clearAndSetSemantics {},
                contentAlignment = Alignment.BottomCenter,
            ) {
                RemotePicture(shown.image, PictureSize.Background, Modifier.fillMaxSize(), pause, loaded = { if (index == 0) firstPicture() })
                Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0.35f to Color.Transparent, 1f to shade.copy(alpha = 0.9f))))
                Box(Modifier.fillMaxSize().background(Brush.horizontalGradient(0f to shade.copy(alpha = 0.7f), 0.6f to Color.Transparent)))
                Column(
                    Modifier.padding(start = 20.dp, end = 20.dp, bottom = 22.dp).widthIn(max = 640.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    if (shown.logo != null) {
                        RemotePicture(shown.logo, PictureSize.Logo, Modifier.widthIn(max = minOf(width - 40.dp, 320.dp)).height(if (tv) 64.dp else 96.dp).fillMaxWidth(), pause, ContentScale.Fit) {
                            BannerTitle(shown.title)
                        }
                    } else {
                        BannerTitle(shown.title)
                    }
                    if (shown.facts.isNotEmpty()) Text(shown.facts, style = Type.font(13.sp, FontWeight.SemiBold, Color(0xFFF2F2F2)), textAlign = TextAlign.Center)
                    if (shown.scores.isNotEmpty()) Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) { shown.scores.forEach { ScoreTag(it) } }
                    if (shown.description.isNotEmpty()) {
                        Text(shown.description, style = Type.font(13.sp, color = Color(0xFFD6D6D6)), textAlign = TextAlign.Center, maxLines = if (tv) 2 else 3)
                    }
                    Row(
                        Modifier.padding(top = 6.dp).height(42.dp).background(palette.accent, RoundedCornerShape(50)).padding(horizontal = 30.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        HavenIcon("info", 18.dp, palette.onAccent)
                        Text("Info", style = Type.font(14.sp, FontWeight.Bold, palette.onAccent))
                    }
                }
            }
        }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            slides.indices.forEach { at ->
                Box(
                    Modifier.size(if (at == index) 30.dp else 8.dp, 8.dp)
                        .background(if (at == index) palette.text else palette.textDim.copy(alpha = 0.5f), RoundedCornerShape(50))
                        // On a TV the dots only show (left and right turn the banner).
                        .then(if (tv) Modifier else Modifier.pressable(RoundedCornerShape(50)) { index = at }),
                )
            }
        }
    }
}

@Composable
private fun BannerTitle(title: String) {
    Text(title, style = Type.font(30.sp, FontWeight.ExtraBold, Color.White), textAlign = TextAlign.Center, maxLines = 2)
}

/** "IMDb 8.4" in a dark pill. */
@Composable
private fun ScoreTag(pill: HomeTop.ScorePill) {
    val tone = when (pill.tone) {
        0 -> Color(0xFFF5C518)
        1 -> Color(0xFF01B4E4)
        2 -> Color(0xFFFA320A)
        else -> Color(0xFF66CC33)
    }
    val shape = RoundedCornerShape(8.dp)
    Row(
        Modifier.height(30.dp).background(Color(0xB80B0D12), shape).border(1.dp, Color.White.copy(alpha = 0.12f), shape).padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(pill.label, style = Type.font(13.sp, FontWeight.ExtraBold, tone))
        Text(pill.value, style = Type.font(14.sp, FontWeight.Bold, Color.White))
    }
}

/** "Today", "In 4 days": gold within a week. */
@Composable
private fun WhenPill(text: String, soon: Boolean, modifier: Modifier = Modifier) {
    val palette = LocalPalette.current
    Box(
        modifier.padding(end = 12.dp, bottom = 8.dp).height(24.dp)
            .background(if (soon) palette.accent else Color.Black.copy(alpha = 0.63f), RoundedCornerShape(50)).padding(horizontal = 7.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, style = Type.font(11.sp, FontWeight.Bold, if (soon) palette.onAccent else Color.White))
    }
}

/** How many episodes are left, in the top corner. */
@Composable
private fun LeftPill(count: Int, modifier: Modifier = Modifier) {
    Box(
        modifier.padding(12.dp).defaultMinSize(28.dp, 28.dp).background(Color.Black.copy(alpha = 0.7f), RoundedCornerShape(50)).padding(horizontal = 8.dp)
            .semantics { contentDescription = if (count == 1) "1 episode left" else "$count episodes left" },
        contentAlignment = Alignment.Center,
    ) {
        Text("$count", style = Type.font(13.sp, FontWeight.ExtraBold, Color.White))
    }
}

/** A shelf: its heading and source, then its posters sideways (or why it
 *  has none). */
@Composable
fun ShelfRow(shelf: Shelf, width: Dp, model: AppModel, counted: Boolean = true) {
    val palette = LocalPalette.current
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(shelf.title, style = Type.heading(18.sp, palette.text))
            Text(shelf.source, style = Type.font(12.sp, color = palette.textDim))
        }
        when {
            shelf.loading && shelf.items.isEmpty() -> Text("Loading…", Modifier.heightIn(min = 40.dp), style = Type.font(14.sp, color = palette.textDim))
            shelf.error.isNotEmpty() && shelf.items.isEmpty() -> Text(shelf.error, style = Type.font(14.sp, color = palette.textDim))
            shelf.items.isEmpty() -> Text("Nothing here.", style = Type.font(14.sp, color = palette.textDim))
            else -> PosterRow(shelf.items, width, model, if (counted) model.library.homePause else null)
        }
    }
}

/** Posters sideways, 3.15 across a phone (160 dp each on a tablet). */
@Composable
fun PosterRow(items: List<MetaPreview>, width: Dp, model: AppModel, pause: Pause?) {
    val card = if (LocalTV.current) PosterFit.TV_POSTER.dp else PosterFit.row(width.value).dp
    val library = model.library
    @Suppress("UNUSED_VARIABLE") val marks = library.marks
    LazyRow(Modifier.tvRow(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        items(items, key = { it.kind + it.id }) { meta ->
            PosterCard(
                meta.name, meta.poster, card,
                Modifier.pressable(RoundedCornerShape(10.dp)) { model.open(Opening(meta.id, meta.kind, meta.name, poster = meta.poster)) },
                rating = meta.rating, watched = library.filmWatched(meta.kind, meta.id), pause = pause,
            )
        }
    }
}

/** A row the remote moves along (on a TV): coming back into it, the remote
 *  lands on the card it was last on, and it stops at the row's ends. */
@OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)
@Composable
fun Modifier.tvRow(): Modifier = if (!LocalTV.current) this else this
    // Left and right stop at the row's ends (not a card in another row).
    .focusProperties {
        exit = { if (it == androidx.compose.ui.focus.FocusDirection.Left || it == androidx.compose.ui.focus.FocusDirection.Right) androidx.compose.ui.focus.FocusRequester.Cancel else androidx.compose.ui.focus.FocusRequester.Default }
    }
    .focusRestorer()
