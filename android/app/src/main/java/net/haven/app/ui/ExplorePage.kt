package net.haven.app.ui

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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import net.haven.app.BuildConfig
import net.haven.app.app.AppModel
import net.haven.app.library.Explore
import net.haven.app.library.Opening

/** Explore: a search box over everything, and a grid to browse one source at
 *  a time (TMDB, a media server's library, a catalogue). The list of sources
 *  opens from the "Sources ›" chip. */
@Composable
fun ExplorePage(model: AppModel) {
    val library = model.library
    val explore = library.explore
    val palette = LocalPalette.current
    val tv = LocalTV.current
    val gutter = if (tv) 0.dp else 16.dp
    LaunchedEffect(Unit) { explore.appeared() }
    Column(Modifier.fillMaxSize().padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(Modifier.padding(horizontal = gutter), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            PageTitle("Explore")
            SearchBox(explore, Modifier.weight(1f), search = { library.runSearch(it) }, clear = { library.clearSearch() })
        }
        when {
            explore.searching -> BoxWithConstraints(Modifier.fillMaxSize()) {
                val width = maxWidth - gutter * 2
                LazyColumn(contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                    if (library.searchNote.isNotEmpty()) item { Text(library.searchNote, style = Type.font(14.sp, color = palette.textDim)) }
                    items(library.searchBoard.shelves, key = { "${it.id}|${it.title}|${it.source}" }) { ShelfRow(it, width, model, counted = false) }
                }
            }
            explore.sources.isEmpty() -> Text(emptyText(BuildConfig.ADDON_SCREENS), Modifier.padding(horizontal = gutter), style = Type.font(14.sp, color = palette.textDim))
            else -> {
                Chip(if (explore.picking) "‹  Back to the titles" else "Sources  ›", explore.picking, Modifier.padding(horizontal = gutter)) {
                    explore.picking = !explore.picking
                }
                if (explore.picking) SourceList(explore, gutter) else TitleGrid(model, explore, gutter)
            }
        }
    }
}

fun emptyText(addons: Boolean) = if (addons) {
    "Explore browses everything you can watch. Add a media server or a TMDB key in Settings, or install an add-on, and it shows here."
} else {
    "Explore browses everything you can watch. Add a media server or a TMDB key in Settings and it shows here."
}

/** A one-line text box in Haven's look. */
@Composable
fun HavenField(
    text: String,
    onChange: (String) -> Unit,
    placeholder: String,
    modifier: Modifier = Modifier,
    imeAction: ImeAction = ImeAction.Done,
    onSubmit: () -> Unit = {},
    leading: @Composable (() -> Unit)? = null,
    trailing: @Composable (() -> Unit)? = null,
) {
    val palette = LocalPalette.current
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(50)
    Row(
        modifier.height(if (LocalTV.current) 52.dp else 40.dp).background(palette.surface, shape)
            .border(1.dp, if (focused) palette.accent else palette.border, shape).padding(start = 14.dp, end = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        leading?.invoke()
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (text.isEmpty()) Text(placeholder, style = Type.font(14.sp, color = palette.textDim), maxLines = 1)
            BasicTextField(
                text, onChange,
                Modifier.fillMaxWidth().onFocusChanged { focused = it.isFocused }.semantics { contentDescription = placeholder },
                textStyle = Type.font(14.sp, color = palette.text),
                singleLine = true,
                cursorBrush = SolidColor(palette.accent),
                keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, imeAction = imeAction),
                keyboardActions = KeyboardActions(onAny = { onSubmit() }),
            )
        }
        trailing?.invoke()
    }
}

/** The round search box: Enter searches; with nothing typed, Paste pastes
 *  and searches at once; ✕ clears the search. */
@Composable
private fun SearchBox(explore: Explore, modifier: Modifier, search: (String) -> Unit, clear: () -> Unit) {
    val palette = LocalPalette.current
    val clipboard = LocalClipboardManager.current
    val focus = LocalFocusManager.current
    fun run(text: String) {
        explore.searching = text.isNotBlank()
        search(text)
        focus.clearFocus()
    }
    HavenField(
        explore.query, { explore.query = it }, "Search films and series", modifier,
        imeAction = ImeAction.Search, onSubmit = { run(explore.query) },
        leading = { HavenIcon("search", 18.dp, palette.textDim) },
        trailing = {
            if (explore.searching || explore.query.isNotEmpty()) {
                Box(Modifier.size(28.dp).pressable(RoundedCornerShape(50)) {
                    explore.query = ""
                    explore.searching = false
                    clear()
                }.semantics { contentDescription = "Clear the search" }, contentAlignment = Alignment.Center) {
                    HavenIcon("close", 14.dp, palette.textDim)
                }
            } else if (!LocalTV.current) {
                Box(Modifier.size(28.dp).background(palette.selected, RoundedCornerShape(50)).pressable(RoundedCornerShape(50)) {
                    val pasted = clipboard.getText()?.text?.trim().orEmpty()
                    if (pasted.isNotEmpty()) {
                        explore.query = pasted
                        run(pasted)
                    }
                }.semantics { contentDescription = "Paste" }, contentAlignment = Alignment.Center) {
                    HavenIcon("paste", 14.dp, palette.text)
                }
            }
        },
    )
}

/** Where to browse: every source, under where it comes from, with a box to
 *  find one. */
@Composable
private fun SourceList(explore: Explore, gutter: androidx.compose.ui.unit.Dp) {
    val palette = LocalPalette.current
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        HavenField(explore.words, { explore.findSource(it) }, "Find a source", Modifier.padding(horizontal = gutter).fillMaxWidth())
        LazyColumn(contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            items(explore.rows, key = { it.id }) { row ->
                if (row.header) {
                    Text(row.label, Modifier.padding(start = 4.dp, end = 4.dp, top = 12.dp, bottom = 4.dp), style = Type.font(11.sp, FontWeight.Bold, palette.accent), maxLines = 1)
                } else {
                    val chosen = row.index == explore.source
                    val shape = RoundedCornerShape(10.dp)
                    Box(
                        Modifier.fillMaxWidth().heightIn(min = 40.dp).background(if (chosen) palette.selected else Color.Transparent, shape)
                            .pressable(shape) { explore.chooseSource(row.index) }.padding(horizontal = 12.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        Text(row.label, style = Type.font(15.sp, if (chosen) FontWeight.SemiBold else FontWeight.Normal, palette.text), maxLines = 1)
                    }
                }
            }
        }
    }
}

/** The chosen source's titles, three across a phone, with Films/Series and
 *  the genre above them. Pull down to load it afresh. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TitleGrid(model: AppModel, explore: Explore, gutter: androidx.compose.ui.unit.Dp) {
    val palette = LocalPalette.current
    val scope = rememberCoroutineScope()
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (explore.kinds.isNotEmpty() || explore.genres.isNotEmpty()) {
            LazyRow(contentPadding = PaddingValues(horizontal = gutter), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                items(explore.kinds.size) { index -> Chip(explore.kinds[index], explore.kind == index) { explore.chooseKind(index) } }
                if (explore.genres.isNotEmpty()) item { GenreMenu(explore) }
            }
        }
        if (explore.note.isNotEmpty() || (explore.loading && explore.items.isEmpty())) {
            Text(explore.note.ifEmpty { "Loading…" }, Modifier.padding(horizontal = gutter), style = Type.font(14.sp, color = palette.textDim), maxLines = 2)
        }
        BoxWithConstraints(Modifier.fillMaxSize()) {
            val (count, card) = PosterFit.grid(maxWidth.value + if (gutter.value == 0f) 32f else 0f)
            @Suppress("UNUSED_VARIABLE") val marks = model.library.marks
            PullToRefreshBox(explore.refreshing, onRefresh = { scope.launch { explore.refresh() } }) {
                LazyVerticalGrid(
                    GridCells.Fixed(count),
                    contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 24.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    items(explore.items, key = { it.kind + "|" + it.id }) { meta ->
                        PosterCard(
                            meta.name, meta.poster, card.dp,
                            Modifier.pressable(RoundedCornerShape(10.dp)) { model.open(Opening(meta.id, meta.kind, meta.name, poster = meta.poster)) },
                            rating = meta.rating, watched = model.library.filmWatched(meta.kind, meta.id),
                        )
                    }
                    if (explore.more && explore.items.isNotEmpty()) item(span = { GridItemSpan(maxLineSpan) }) {
                        Row(Modifier.padding(top = 8.dp)) {
                            Chip(if (explore.loading) "Loading…" else "Load more", false, enabled = !explore.loading) { explore.load() }
                        }
                    }
                }
            }
        }
    }
}

/** The genre chooser: the first choice stands for every genre. */
@Composable
private fun GenreMenu(explore: Explore) {
    val palette = LocalPalette.current
    var open by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(50)
    Box {
        Row(
            Modifier.height(34.dp).background(palette.surface, shape).border(1.dp, palette.border, shape)
                .pressable(shape) { open = true }.semantics { contentDescription = "Genre" }.padding(horizontal = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(explore.genres.getOrNull(explore.genre + 1) ?: "All genres", style = Type.font(14.sp, FontWeight.SemiBold, palette.text), maxLines = 1)
            HavenIcon("forward", 12.dp, palette.textDim, modifier = Modifier.rotate(90f))
        }
        DropdownMenu(open, { open = false }) {
            explore.genres.forEachIndexed { choice, name ->
                DropdownMenuItem(
                    { Text(name, style = Type.font(15.sp, if (choice == explore.genre + 1) FontWeight.Bold else FontWeight.Normal)) },
                    {
                        open = false
                        explore.chooseGenre(choice)
                    },
                    leadingIcon = { if (choice == explore.genre + 1) HavenIcon("check", 16.dp) else Spacer(Modifier.width(16.dp)) },
                )
            }
        }
    }
}

/** A round choice: gold when chosen. */
@Composable
fun Chip(label: String, selected: Boolean, modifier: Modifier = Modifier, enabled: Boolean = true, onClick: () -> Unit) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(50)
    Box(
        modifier.height(if (LocalTV.current) 44.dp else 34.dp)
            .background(if (selected) palette.accent else palette.surface, shape)
            .border(1.dp, if (selected) Color.Transparent else palette.border, shape)
            .pressable(shape, enabled = enabled, onClick = onClick)
            .padding(horizontal = 14.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Type.font(14.sp, FontWeight.SemiBold, if (selected) palette.onAccent else palette.text), maxLines = 1)
    }
}
