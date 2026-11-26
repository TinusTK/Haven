package net.haven.app.ui

import android.content.Intent
import android.net.Uri
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import net.haven.app.app.AppModel
import net.haven.app.library.EpisodeRow
import net.haven.app.library.SourcesPanel
import net.haven.app.library.StreamRow
import net.haven.app.library.TitlePage
import net.haven.app.library.TitleRules
import net.haven.app.metadata.WhereToWatch
import net.haven.app.pictures.PictureSize

/** A title's page: the wide picture behind, the details over it, and under
 *  them (once asked for) a panel with the episodes, then the places to
 *  watch. The details take the top part while the panel is open. */
@Composable
fun TitlePageView(model: AppModel) {
    val page = model.library.title
    val palette = LocalPalette.current
    BoxWithConstraints(Modifier.fillMaxSize().background(palette.background)) {
        val top = if (page.panelOpen) minOf(maxHeight * 0.4f, if (LocalTV.current) 300.dp else 330.dp) else maxHeight
        RemotePicture(page.background, PictureSize.Background, Modifier.fillMaxSize())
        Box(Modifier.fillMaxSize().background(Brush.horizontalGradient(0f to palette.background.copy(alpha = 0.94f), 0.55f to palette.background.copy(alpha = 0.6f), 1f to palette.background.copy(alpha = 0.4f))))
        Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0.35f to Color.Transparent, 1f to palette.background.copy(alpha = 0.9f))))
        Box(Modifier.fillMaxWidth().height(top).safeDrawingPadding()) { Details(model, page, top) }
        AnimatedVisibility(page.panelOpen, enter = slideInVertically { it / 3 } + fadeIn(), exit = slideOutVertically { it / 3 } + fadeOut()) {
            val shape = RoundedCornerShape(18.dp)
            Box(
                Modifier.offset(y = top).padding(horizontal = 12.dp).fillMaxWidth().height(maxOf(maxHeight - top - 10.dp, 0.dp))
                    .clip(shape).background(palette.surface.copy(alpha = 0.96f)).border(1.dp, palette.border, shape),
            ) {
                if (page.sourcesShown) SourcesList(page, page.sources, model.cast) else EpisodeList(page)
            }
        }
    }
}

/** The title, on the left over the picture; scrolls when it does not fit. */
@Composable
private fun Details(model: AppModel, page: TitlePage, height: Dp) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    val first = remember { FocusRequester() }
    LaunchedEffect(page.id) { if (tv) runCatching { first.requestFocus() } }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = if (tv) 48.dp else 16.dp, vertical = 10.dp).widthIn(max = 720.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(
            Modifier.height(36.dp).background(palette.surface.copy(alpha = 0.9f), RoundedCornerShape(50)).border(1.dp, palette.border, RoundedCornerShape(50))
                .pressable(RoundedCornerShape(50)) { model.closeTitle() }.semantics { contentDescription = "Back" }.padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            HavenIcon("back", 16.dp, palette.text)
            Text("Back", style = Type.font(14.sp, FontWeight.SemiBold, palette.text))
        }
        Spacer(Modifier.weight(1f, fill = false).heightIn(min = 0.dp))
        if (!page.panelOpen && height > 520.dp && page.poster != null) {
            RemotePicture(page.poster, PictureSize.Poster, Modifier.size(110.dp, 165.dp).clip(RoundedCornerShape(10.dp)).border(1.dp, palette.border, RoundedCornerShape(10.dp)))
        }
        Text(page.title, style = Type.heading(24.sp, palette.text))
        if (page.film || page.watched) FilmMark(page)
        if (page.episodeOpen && page.episodeLabel.isNotEmpty()) Text(page.episodeLabel, style = Type.font(18.sp, FontWeight.SemiBold, palette.accent))
        if (page.facts.isNotEmpty()) Text(page.facts, style = Type.font(14.sp, FontWeight.SemiBold, palette.text))
        if (page.loading) Text("Loading details…", style = Type.font(14.sp, color = palette.textDim))
        if (page.error.isNotEmpty()) Text(page.error, style = Type.font(14.sp, color = palette.textDim))
        if (page.description.isNotEmpty()) {
            Text(page.description, style = Type.font(14.sp, color = palette.text), maxLines = if (page.panelOpen) 3 else 7, overflow = TextOverflow.Ellipsis)
        }
        if (page.cast.isNotEmpty() && !page.panelOpen) Text("Cast  ·  ${page.cast}", style = Type.font(13.sp, color = palette.textDim), maxLines = 2)
        WhereToWatchRow(page)
        Row(Modifier.padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            if (!page.isSeries && !page.loading) PrimaryButton("▶  Sources", Modifier.focusRequester(first)) { page.showFilmSources() }
            if (page.isSeries && page.seasons.isEmpty()) PrimaryButton("Episodes", Modifier.focusRequester(first)) { page.panelOpen = true }
            Chip(if (page.saved) "✓  On Watchlist" else "＋  Add to Watchlist", page.saved, if (page.isSeries && page.seasons.isNotEmpty()) Modifier.focusRequester(first) else Modifier) {
                page.toggleWatchlist()
            }
        }
        if (page.seasons.isNotEmpty() && !page.panelOpen) SeasonChips(page, selected = false)
    }
}

/** A film: the pill marks it watched or unwatched. */
@Composable
private fun FilmMark(page: TitlePage) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(50)
    Row(
        Modifier.height(30.dp).background(if (page.watched) palette.accent.copy(alpha = 0.14f) else Color.Transparent, shape)
            .border(1.5.dp, if (page.watched) palette.accent else palette.textDim.copy(alpha = 0.6f), shape)
            .pressable(shape) { page.markFilm(!page.watched) }
            .semantics { contentDescription = if (page.watched) "Watched. Mark unwatched" else "Mark watched" }
            .padding(start = 6.dp, end = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        if (page.watched) WatchedTick(20.dp) else Box(Modifier.size(20.dp).border(1.5.dp, palette.textDim, CircleShape))
        Text(if (page.watched) "Watched" else "Mark watched", style = Type.font(13.sp, FontWeight.Bold, if (page.watched) palette.accent else palette.text))
    }
}

/** Where to watch: the services that have it, grouped Stream / Free / Rent /
 *  Buy, on Haven's own tiles. A tile opens JustWatch. */
@Composable
private fun WhereToWatchRow(page: TitlePage) {
    val palette = LocalPalette.current
    val context = LocalContext.current
    if (page.offers.isNotEmpty()) {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Where to watch  ·  ${page.offersNote}", style = Type.font(12.sp, color = palette.textDim), maxLines = 1)
            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                itemsIndexed(page.offers) { index, tile ->
                    Column(
                        Modifier.padding(start = if (tile.group.isNotEmpty() && index > 0) 16.dp else 0.dp)
                            .pressable(RoundedCornerShape(12.dp)) {
                                if (page.offersLink.isNotEmpty()) runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(page.offersLink))) }
                            }
                            .semantics { contentDescription = "${tile.name}, ${tile.ways}. Opens JustWatch" },
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(tile.group, Modifier.height(14.dp), style = Type.font(11.sp, FontWeight.SemiBold, palette.accent))
                        Box(Modifier.size(52.dp).background(Color(0xFF000000 or tile.tint.toLong()), RoundedCornerShape(12.dp)).border(1.dp, Color.White.copy(alpha = 0.08f), RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
                            Text(tile.letters, style = Type.font(if (tile.letters.length > 2) 13.sp else 17.sp, FontWeight.ExtraBold, Color(0xFF000000 or tile.ink.toLong())))
                        }
                        Text(tile.name, Modifier.width(56.dp), style = Type.font(11.sp, color = palette.textDim), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            Text(WhereToWatch.CREDIT_NOTE, style = Type.font(10.sp, color = palette.textDim.copy(alpha = 0.8f)))
        }
    } else if (page.offersNote.isNotEmpty() && !page.loading) {
        Text(page.offersNote, style = Type.font(12.sp, color = palette.textDim))
    }
}

/** The seasons; picking one lists its episodes in the panel. */
@Composable
private fun SeasonChips(page: TitlePage, selected: Boolean) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(page.seasons) { number ->
            Chip(if (number == 0) "Specials" else "Season $number", selected && number == page.season) {
                if (selected) page.showSeason(number) else page.chooseSeason(number)
            }
        }
    }
}

@Composable
private fun PanelHeading(title: String, page: TitlePage, leading: @Composable () -> Unit = {}) {
    val palette = LocalPalette.current
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        leading()
        Text(title, Modifier.weight(1f), style = Type.heading(16.sp, palette.text), maxLines = 1, overflow = TextOverflow.Ellipsis)
        Box(
            Modifier.size(32.dp).background(palette.background.copy(alpha = 0.5f), CircleShape).pressable(CircleShape) {
                page.panelOpen = false
                page.episodeOpen = false
            }.semantics { contentDescription = "Close the list" },
            contentAlignment = Alignment.Center,
        ) { HavenIcon("close", 14.dp, palette.textDim) }
    }
}

@Composable
private fun EpisodeList(page: TitlePage) {
    val palette = LocalPalette.current
    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        PanelHeading(if (page.seasons.isEmpty()) "Episodes" else if (page.season == 0) "Specials" else "Season ${page.season}", page)
        if (page.seasons.isNotEmpty()) {
            SeasonChips(page, selected = true)
            if (page.episodes.isNotEmpty() || page.episodeFilter.isNotEmpty()) {
                Chip(if (page.seasonAllWatched) "✓  Season watched · mark unwatched" else "Mark season watched", page.seasonAllWatched) {
                    page.markSeason(!page.seasonAllWatched)
                }
            }
            HavenField(page.episodeFilter, { page.filterEpisodes(it) }, "Filter episodes", Modifier.fillMaxWidth())
        }
        if (page.episodes.isEmpty()) Text("No episode matches.", style = Type.font(14.sp, color = palette.textDim))
        LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp), contentPadding = PaddingValues(bottom = 12.dp)) {
            items(page.episodes, key = { it.id }) { episode ->
                EpisodeItem(episode, open = { page.openEpisode(episode.id) }, toggle = { page.markEpisodes(listOf(episode.id), !episode.watched) })
            }
        }
    }
}

/** One episode: its number, name and date, and the round mark. With a still
 *  the picture comes first, the mark on its corner; without one the mark
 *  sits at the end. */
@Composable
private fun EpisodeItem(episode: EpisodeRow, open: () -> Unit, toggle: () -> Unit) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(8.dp)
    val back = if (episode.selected) palette.selected else palette.surface
    @Composable fun Mark() {
        Box(
            Modifier.size(30.dp, if (episode.picture == null) 44.dp else 30.dp).pressable(CircleShape, onClick = toggle)
                .semantics { contentDescription = if (episode.watched) "Watched. Mark unwatched" else "Mark watched" },
            contentAlignment = Alignment.Center,
        ) {
            if (episode.watched) {
                WatchedTick(24.dp)
            } else {
                Box(
                    Modifier.size(24.dp).background(if (episode.picture == null) Color.Transparent else Color.Black.copy(alpha = 0.35f), CircleShape)
                        .border(1.5.dp, if (episode.picture == null) palette.textDim.copy(alpha = 0.5f) else Color.White.copy(alpha = 0.7f), CircleShape),
                )
            }
        }
    }
    if (episode.picture != null) {
        Box(Modifier.fillMaxWidth().height(78.dp).background(back, shape).border(1.dp, palette.border, shape)) {
            Row(
                Modifier.fillMaxSize().pressable(shape, onClick = open).semantics { contentDescription = "${episode.number} ${episode.title}" }.padding(7.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RemotePicture(episode.picture, PictureSize.Wide, Modifier.size(114.dp, 64.dp).clip(RoundedCornerShape(6.dp)).background(palette.background))
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(if (episode.number.isEmpty()) episode.title else "${episode.number} · ${episode.title}", style = Type.font(14.sp, FontWeight.SemiBold, palette.text), maxLines = 2)
                    Text(episode.date, style = Type.font(12.sp, color = palette.textDim))
                }
            }
            Box(Modifier.padding(start = 7.dp + 114.dp - 30.dp + 2.dp, top = 7.dp)) { Mark() }
        }
    } else {
        Row(
            Modifier.fillMaxWidth().height(44.dp).background(back, shape).border(1.dp, palette.border, shape).padding(start = 14.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                Modifier.weight(1f).fillMaxSize().pressable(RoundedCornerShape(6.dp), onClick = open).semantics { contentDescription = "${episode.number} ${episode.title}" },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(episode.number, Modifier.width(44.dp), style = Type.font(14.sp, FontWeight.SemiBold, palette.accent))
                Text(episode.title, Modifier.weight(1f), style = Type.font(14.sp, color = palette.text), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(episode.date, style = Type.font(12.sp, color = palette.textDim))
            }
            Mark()
        }
    }
}

@Composable
private fun SourcesList(page: TitlePage, sources: SourcesPanel, cast: net.haven.app.cast.CastModel) {
    val palette = LocalPalette.current
    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        PanelHeading(if (page.isSeries && page.episodeLabel.isNotEmpty()) page.episodeLabel else TitleRules.HEADING, page) {
            if (page.isSeries) Chip("‹", false, Modifier.semantics { contentDescription = "Back to the episodes" }) { page.episodeOpen = false }
        }
        if (page.playNote.isNotEmpty()) Text(page.playNote, style = Type.font(13.sp, color = palette.accent))
        if (page.resumeNote.isNotEmpty()) Text(page.resumeNote, style = Type.font(13.sp, color = palette.accent))
        if (sources.groups.size > 1) {
            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                item { Chip("All  ${sources.rows.size}", sources.group.isEmpty()) { sources.group = "" } }
                items(sources.groups, key = { it.first }) { (name, count) -> Chip("$name  $count", sources.group == name) { sources.group = name } }
            }
        }
        if (page.loading && !sources.loading) Text("Loading…", style = Type.font(13.sp, color = palette.textDim))
        if (sources.loading) Text(if (sources.rows.isEmpty()) TitleRules.ASKING else "More are still coming in…", style = Type.font(13.sp, color = palette.textDim))
        if (sources.note.isNotEmpty()) Text(sources.note, style = Type.font(14.sp, color = palette.textDim))
        LazyColumn(Modifier.weight(1f, fill = false), verticalArrangement = Arrangement.spacedBy(8.dp), contentPadding = PaddingValues(bottom = 8.dp)) {
            items(sources.shownRows, key = { it.id }) { row ->
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (row.heading.isNotEmpty()) Text(row.heading, Modifier.padding(top = 6.dp), style = Type.font(13.sp, FontWeight.Bold, palette.accent))
                    @Suppress("UNUSED_VARIABLE") val seen = page.downloadsRevision
                    SourceCard(row, if (row.kind == "Direct" && row.playable) page.savedState(row) else null, download = { page.download(row) }) { page.play(row) }
                    if (row.more > 0) Chip("Show more from ${row.group}  (${row.more} left)", false) { sources.showMore(row.group) }
                }
            }
        }
        PlayOn(cast)
    }
}

/** Where it plays: this device, or a Chromecast found on the network. */
@Composable
private fun PlayOn(cast: net.haven.app.cast.CastModel) {
    val palette = LocalPalette.current
    if (LocalTV.current || !cast.available) return
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(palette.border))
        LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            item { Text("Play on", style = Type.font(13.sp, color = palette.textDim)) }
            item { Chip("This device", cast.target < 0) { cast.target = -1 } }
            items(cast.devices.size) { at -> Chip(cast.devices[at].name, cast.target == at) { cast.target = at } }
            item { Chip(if (cast.finding) "Looking…" else "Chromecast…", false, enabled = !cast.finding) { cast.find() } }
        }
        if (cast.note.isNotEmpty()) Text(cast.note, style = Type.font(12.sp, color = palette.textDim))
    }
}

/** A place to watch: its name, size and how many share it, the details,
 *  where it came from, and Play (or how it is delivered, when Haven cannot
 *  play it). */
@Composable
private fun SourceCard(row: StreamRow, saved: Int?, download: () -> Unit, play: () -> Unit) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(12.dp)
    Row(
        Modifier.fillMaxWidth().heightIn(min = 88.dp).background(palette.background.copy(alpha = 0.55f), shape).border(1.dp, palette.border, shape)
            .pressable(shape, enabled = row.playable, onClick = play)
            .semantics { contentDescription = "${if (row.playable) "Play " else ""}${row.name}, ${row.kind}, from ${row.source}" }
            .padding(horizontal = 14.dp, vertical = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
                Text(row.name, Modifier.weight(1f), style = Type.font(14.sp, FontWeight.Bold, palette.text))
                if (row.size.isNotEmpty()) {
                    Box(Modifier.height(20.dp).background(palette.accent.copy(alpha = 0.15f), RoundedCornerShape(50)).padding(horizontal = 7.dp), contentAlignment = Alignment.Center) {
                        Text(row.size, style = Type.font(11.sp, FontWeight.Bold, palette.accent))
                    }
                }
                if (row.peers.isNotEmpty()) {
                    Row(
                        Modifier.height(20.dp).background(palette.textDim.copy(alpha = 0.15f), RoundedCornerShape(50)).padding(horizontal = 7.dp)
                            .semantics { contentDescription = "${row.peers} sharing" },
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(3.dp),
                    ) {
                        HavenIcon("account", 12.dp, palette.text, stroke = 2.2.dp)
                        Text(row.peers, style = Type.font(11.sp, FontWeight.Bold, palette.text))
                    }
                }
            }
            if (row.detail.isNotEmpty()) Text(row.detail, style = Type.font(12.sp, color = palette.textDim))
            if (row.detail2.isNotEmpty()) Text(row.detail2, style = Type.font(12.sp, color = palette.textDim))
            Text(if (row.group == row.source) row.source else "${row.group}  ·  ${row.source}", style = Type.font(11.sp, color = palette.textDim), maxLines = 1)
        }
        if (saved != null) {
            // Filled once it is downloaded (or on its way): pressing it again
            // plays the saved copy.
            Box(
                Modifier.size(38.dp).background(if (saved > 0) palette.accent else palette.selected, CircleShape).pressable(CircleShape, onClick = download)
                    .semantics { contentDescription = when (saved) { 2 -> "Play the downloaded copy of ${row.name}"; 1 -> "Downloading ${row.name}"; else -> "Download ${row.name}" } },
                contentAlignment = Alignment.Center,
            ) { HavenIcon("download", 18.dp, if (saved > 0) palette.onAccent else palette.accent) }
        }
        if (row.playable) {
            Box(Modifier.size(38.dp).background(palette.selected, CircleShape), contentAlignment = Alignment.Center) {
                HavenIcon("play", 14.dp, palette.accent)
            }
        } else {
            Text(row.kind, style = Type.font(12.sp, color = palette.textDim))
        }
    }
}
