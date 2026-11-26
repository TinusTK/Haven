package net.haven.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.border
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import net.haven.app.app.AppModel
import net.haven.app.app.Page
import net.haven.app.livetv.LiveTVModel

/** Live TV: television or radio, the chosen category's channels, and (on
 *  asking) the categories with the Favourites groups first. */
@Composable
fun LiveTVPage(model: AppModel) {
    val live = model.live
    val palette = LocalPalette.current
    val gutter = if (LocalTV.current) 0.dp else 16.dp
    Column(Modifier.fillMaxSize().padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = gutter), verticalAlignment = Alignment.CenterVertically) {
            PageTitle("Live TV")
            Spacer(Modifier.weight(1f))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("TV", "Radio").forEachIndexed { at, label -> Chip(label, live.band == at) { live.chooseBand(at) } }
            }
        }
        if (live.board.channels.isEmpty()) {
            Column(Modifier.padding(horizontal = gutter), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    if (live.isRadio) "Stations marked as radio in your Live TV sources show here."
                    else "Add an M3U playlist or an Xtream Codes account in Settings, under Connections › Live TV, to see the channels here.",
                    style = Type.font(14.sp, color = palette.textDim),
                )
                if (!live.isRadio) PrimaryButton("Open Settings") { model.select(Page.Settings) }
            }
        } else {
            Chip(
                if (live.picking) "‹  Back to the channels" else "${live.categoryName}  ›", live.picking,
                Modifier.padding(horizontal = gutter).semantics { contentDescription = if (live.picking) "Back to the channels" else "Categories, now ${live.categoryName}" },
            ) { live.picking = !live.picking }
            if (live.picking) CategoryList(live, gutter) else ChannelList(live, gutter)
        }
    }
    if (live.choosing != null) FavouritesDialog(live)
}

/** The categories: Favourites groups (with ★), every channel, the playlists'
 *  own groups; a box to find one and one to make a group. */
@Composable
private fun CategoryList(live: LiveTVModel, gutter: Dp) {
    val palette = LocalPalette.current
    var newGroup by remember { mutableStateOf("") }
    fun make() {
        live.newGroup(newGroup)
        newGroup = ""
    }
    Column(Modifier.padding(horizontal = gutter), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        HavenField(live.board.categoryWords, { live.setCategoryWords(it) }, "Find a category", Modifier.fillMaxWidth())
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            HavenField(newGroup, { newGroup = it }, "New Favourites group", Modifier.weight(1f), onSubmit = { make() })
            Chip("Add", false, Modifier.semantics { contentDescription = "Add the Favourites group" }, enabled = newGroup.isNotBlank()) { make() }
        }
        LazyColumn(contentPadding = PaddingValues(bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            items(live.categories, key = { it.key }) { row ->
                val chosen = row.key == live.board.category
                val shape = RoundedCornerShape(10.dp)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Row(
                        Modifier.weight(1f).heightIn(min = 42.dp).background(if (chosen) palette.selected else Color.Transparent, shape)
                            .pressable(shape) { live.choose(row.key) }.padding(horizontal = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        if (row.favourite) Text("★", style = Type.font(14.sp, color = palette.accent))
                        Text(row.label, Modifier.weight(1f), style = Type.font(15.sp, if (chosen) FontWeight.SemiBold else FontWeight.Normal, palette.text), maxLines = 1)
                        Text("${row.count}", style = Type.font(12.sp, color = palette.textDim))
                    }
                    if (row.favourite) {
                        Box(
                            Modifier.size(36.dp).pressable(RoundedCornerShape(50)) { live.deleteGroup(row.label) }
                                .semantics { contentDescription = "Delete the Favourites group ${row.label}" },
                            contentAlignment = Alignment.Center,
                        ) { HavenIcon("trash", 16.dp, palette.textDim) }
                    }
                }
            }
        }
    }
}

/** The chosen category's channels, with a filter. */
@Composable
private fun ChannelList(live: LiveTVModel, gutter: Dp) {
    val palette = LocalPalette.current
    val rows = live.channels
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.padding(horizontal = gutter), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            HavenField(live.board.words, { live.setWords(it) }, "Filter by name", Modifier.weight(1f))
            Text(live.countText, style = Type.font(12.sp, color = palette.textDim))
        }
        if (rows.isEmpty()) {
            Text("Nothing here. Use the ☆ on a channel to put it in a Favourites group.", Modifier.padding(horizontal = gutter), style = Type.font(14.sp, color = palette.textDim))
        }
        LazyColumn(contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            items(rows, key = { it.index }) { channel ->
                val shape = RoundedCornerShape(10.dp)
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 52.dp).background(palette.surface, shape).border(1.dp, palette.border, shape).padding(start = 14.dp, end = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Row(
                        Modifier.weight(1f).heightIn(min = 48.dp).pressable(RoundedCornerShape(8.dp)) { live.play(channel.index) }
                            .semantics { contentDescription = "Play ${channel.name}" },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(channel.name, style = Type.font(14.sp, FontWeight.SemiBold, palette.text), maxLines = 1)
                            if (channel.group.isNotEmpty()) Text(channel.group, style = Type.font(12.sp, color = palette.textDim), maxLines = 1)
                        }
                        HavenIcon("play", 13.dp, palette.accent)
                    }
                    Box(
                        Modifier.size(36.dp).pressable(RoundedCornerShape(50)) { live.chooseFavourites(channel.index) }
                            .semantics { contentDescription = if (channel.starred) "Change the Favourites groups of ${channel.name}" else "Add to Favourites: ${channel.name}" },
                        contentAlignment = Alignment.Center,
                    ) { Text(if (channel.starred) "★" else "☆", style = Type.font(20.sp, color = if (channel.starred) palette.accent else palette.textDim)) }
                }
            }
        }
    }
}

/** Choosing the Favourites groups of one channel. */
@Composable
private fun FavouritesDialog(live: LiveTVModel) {
    val palette = LocalPalette.current
    var name by remember { mutableStateOf("") }
    fun add() {
        live.newGroup(name)
        name = ""
    }
    Dialog(onDismissRequest = { live.closeChooser() }) {
        Column(
            Modifier.fillMaxWidth().background(palette.background, RoundedCornerShape(18.dp)).padding(20.dp).heightIn(max = 560.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Favourites", style = Type.heading(20.sp, palette.text))
            Text(live.choosing?.name ?: "", style = Type.font(14.sp, color = palette.textDim), maxLines = 1)
            if (live.favourites.isEmpty()) {
                Text("You have no Favourites groups yet. Name one below and this channel goes into it.", style = Type.font(14.sp, color = palette.textDim))
            }
            Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                live.favourites.forEach { group ->
                    val has = live.has(group)
                    val shape = RoundedCornerShape(10.dp)
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 44.dp).background(if (has) palette.selected else palette.surface, shape)
                            .pressable(shape) { live.toggle(group.name) }.padding(horizontal = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(group.name, Modifier.weight(1f), style = Type.font(15.sp, color = palette.text))
                        if (has) HavenIcon("check", 18.dp, palette.accent)
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                HavenField(name, { name = it }, "New group, for example Sport", Modifier.weight(1f), onSubmit = { add() })
                Chip("New group", false, enabled = name.isNotBlank()) { add() }
            }
            PrimaryButton("Done", Modifier.width(200.dp)) { live.closeChooser() }
        }
    }
}
