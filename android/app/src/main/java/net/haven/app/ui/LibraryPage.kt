package net.haven.app.ui

import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import net.haven.app.app.AppModel
import net.haven.app.library.DownloadRow
import net.haven.app.library.DownloadRules
import net.haven.app.library.Downloads
import net.haven.app.library.LocalVideos
import net.haven.app.library.Opening
import net.haven.app.player.Resume
import java.io.File

/** The Library: videos on this device (Local), Downloads, the Watchlist and
 *  History, under one switch. */
@Composable
fun LibraryPage(model: AppModel) {
    val library = model.library
    val tv = LocalTV.current
    val gutter = if (tv) 0.dp else 16.dp
    val context = LocalContext.current
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        if (allowed) library.lookForVideos { LocalVideos.find(context) } else library.sayLocal(DownloadRules.ALLOW_LOCAL)
    }
    fun look() {
        if (LocalVideos.allowed(context)) library.lookForVideos { LocalVideos.find(context) } else ask.launch(LocalVideos.permission)
    }
    LaunchedEffect(model.libraryTab) { if (model.libraryTab == 0 && library.localVideos.isEmpty() && !library.localBusy) look() }
    Column(Modifier.fillMaxSize().padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        PageTitle("Library", Modifier.padding(horizontal = gutter))
        LazyRow(Modifier.tvRow(), contentPadding = PaddingValues(horizontal = gutter), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            items(TABS.size) { index -> Chip(TABS[index], model.libraryTab == index) { model.libraryTab = index } }
        }
        when (model.libraryTab) {
            0 -> LocalList(model, gutter) { look() }
            1 -> DownloadList(model, library.downloads, gutter)
            2 -> PosterGrid(model, library.watchlist.map { GridItem(it.titleId, it.kind, it.title, it.year, it.poster, null, 0.0, false) },
                "Nothing saved yet. Open a film or series and choose Add to Watchlist.", removable = false, gutter)
            else -> PosterGrid(model, library.history.map {
                GridItem(it.titleId, it.kind, it.title, DownloadRules.historyNote(it), it.poster, it.videoId,
                    if (it.finished) 1.0 else maxOf(Resume.share(it.position, it.duration).toDouble(), 0.02), it.finished)
            }, "Nothing watched yet. Films and episodes you play show here, newest first.", removable = true, gutter)
        }
    }
}

private val TABS = listOf("Local", "Downloads", "Watchlist", "History")

/** Local: the videos on this device (Android's media list). */
@Composable
private fun LocalList(model: AppModel, gutter: Dp, look: () -> Unit) {
    val library = model.library
    val palette = LocalPalette.current
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.padding(horizontal = gutter), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(DownloadRules.LOCAL_PLACE + if (library.localStatus.isEmpty()) "" else "  ·  ${library.localStatus}", Modifier.weight(1f), style = Type.font(12.sp, color = palette.textDim))
            Chip(if (library.localBusy) "Looking…" else "Look again", false, enabled = !library.localBusy, onClick = look)
        }
        LazyColumn(contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(library.localVideos, key = { it.address }) { found ->
                val shape = RoundedCornerShape(10.dp)
                Row(
                    Modifier.fillMaxWidth().height(56.dp).background(palette.surface, shape).border(1.dp, palette.border, shape)
                        .pressable(shape) { library.playLocal(found) }.semantics { contentDescription = "Play ${found.name}" }.padding(horizontal = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    HavenIcon("play", 18.dp, palette.accent)
                    Column {
                        Text(found.name, style = Type.font(14.sp, FontWeight.SemiBold, palette.text), maxLines = 1)
                        Text(found.detail, style = Type.font(12.sp, color = palette.textDim), maxLines = 1)
                    }
                }
            }
        }
    }
}

/** Downloads: films, then series by name. */
@Composable
private fun DownloadList(model: AppModel, downloads: Downloads, gutter: Dp) {
    val palette = LocalPalette.current
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.padding(horizontal = gutter), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            HavenIcon("folder", 18.dp, palette.textDim)
            Text("Saved in ${DownloadRules.placeWords(downloads.root)}", style = Type.font(12.sp, color = palette.textDim), maxLines = 1)
        }
        if (downloads.rows.isEmpty()) {
            Text(DownloadRules.NOTHING_DOWNLOADED, Modifier.padding(horizontal = gutter), style = Type.font(14.sp, color = palette.textDim))
            return
        }
        LazyColumn(contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(downloads.rows, key = { it.id }) { row ->
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (row.section.isNotEmpty()) Text(row.section, Modifier.padding(top = 6.dp), style = Type.heading(18.sp, palette.text))
                    if (row.group.isNotEmpty()) Text(row.group, style = Type.font(13.sp, FontWeight.Bold, palette.accent))
                    DownloadItem(row, play = { downloads.playRequest(row.id)?.let { model.library.play?.invoke(it) } }, toggle = { downloads.toggle(row.id) }, remove = { downloads.remove(row.id) })
                }
            }
        }
    }
}

/** One download: its name, how far it is, and play / pause or carry on / delete. */
@Composable
private fun DownloadItem(row: DownloadRow, play: () -> Unit, toggle: () -> Unit, remove: () -> Unit) {
    val palette = LocalPalette.current
    var asking by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(10.dp)
    Row(
        Modifier.fillMaxWidth().background(palette.surface, shape).border(1.dp, palette.border, shape).padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        row.picture?.let { KeptPicture(it, Modifier.size(80.dp, 45.dp).clip(RoundedCornerShape(6.dp))) }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Text(row.title, style = Type.font(14.sp, FontWeight.SemiBold, palette.text), maxLines = 1)
            Text(row.status, style = Type.font(12.sp, color = if (row.failed) palette.alert else palette.textDim), maxLines = 1)
            if (!row.done) ProgressBar(row.progress, track = palette.border)
        }
        if (row.done) Round("play", "Play ${row.title}", play) else Round(if (row.running) "pause" else "download", if (row.running) "Pause" else "Carry on", toggle)
        Round("trash", "Delete ${row.title}") { asking = true }
    }
    if (asking) {
        AlertDialog(
            onDismissRequest = { asking = false },
            title = { Text("Delete ${row.title}?", style = Type.font(17.sp, FontWeight.Bold)) },
            text = { Text("Its file is deleted from this device.", style = Type.font(14.sp)) },
            confirmButton = { TextButton({ asking = false; remove() }) { Text("Delete the download", color = palette.alert) } },
            dismissButton = { TextButton({ asking = false }) { Text("Keep it") } },
        )
    }
}

@Composable
private fun Round(icon: String, name: String, onClick: () -> Unit) {
    val palette = LocalPalette.current
    Box(Modifier.size(38.dp).background(palette.selected, CircleShape).pressable(CircleShape, onClick = onClick).semantics { contentDescription = name }, contentAlignment = Alignment.Center) {
        HavenIcon(icon, 17.dp, palette.text)
    }
}

/** A picture kept on this device (a download's). */
@Composable
private fun KeptPicture(file: File, modifier: Modifier) {
    val image = remember(file.path, file.lastModified()) {
        runCatching { BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = 2 })?.asImageBitmap() }.getOrNull()
    }
    if (image != null) Image(image, null, modifier, contentScale = ContentScale.Crop) else Box(modifier.background(LocalPalette.current.background))
}

/** What a Watchlist or History poster shows. `note` is the year, or for
 *  History the episode or how much is left. */
private data class GridItem(
    val id: String,
    val kind: String,
    val title: String,
    val note: String,
    val poster: String?,
    val video: String?,
    val progress: Double,
    val watched: Boolean,
)

/** Posters three across a phone, more on a tablet; History's can be taken
 *  off with a long press. */
@Composable
private fun PosterGrid(model: AppModel, items: List<GridItem>, empty: String, removable: Boolean, gutter: Dp) {
    val palette = LocalPalette.current
    if (items.isEmpty()) {
        Text(empty, Modifier.padding(horizontal = gutter), style = Type.font(14.sp, color = palette.textDim))
        return
    }
    @Suppress("UNUSED_VARIABLE") val marks = model.library.marks
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val (count, card) = PosterFit.grid(maxWidth.value + if (gutter.value == 0f) 32f else 0f, LocalTV.current)
        LazyVerticalGrid(
            GridCells.Fixed(count),
            contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            itemsIndexed(items, key = { _, item -> item.kind + item.id }) { _, item ->
                var menu by remember { mutableStateOf(false) }
                Box {
                    Column(
                        Modifier.pressable(RoundedCornerShape(10.dp), onLongClick = if (removable) ({ menu = true }) else null) {
                            model.open(Opening(item.id, item.kind, item.title, item.video, item.poster))
                        },
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        PosterCard(item.title, item.poster, card.dp, watched = item.watched || model.library.filmWatched(item.kind, item.id),
                            progress = if (item.watched) 0.0 else item.progress)
                        if (item.note.isNotEmpty()) Text(item.note, Modifier.width(card.dp), style = Type.font(12.sp, color = palette.textDim), maxLines = 1)
                    }
                    DropdownMenu(menu, { menu = false }) {
                        DropdownMenuItem({ Text("Remove from History", style = Type.font(15.sp, color = palette.alert)) }, {
                            menu = false
                            model.library.removeContinue(item.kind, item.id)
                        })
                    }
                }
            }
        }
    }
}
