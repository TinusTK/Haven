package net.haven.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import net.haven.app.app.AppModel
import net.haven.app.library.CalendarRules
import net.haven.app.library.Day
import net.haven.app.library.Opening

/** The Calendar: a month (or week) of squares, a dot for each film or
 *  episode from the Watchlist coming out (a tick once watched), and the
 *  picked day's releases under them. */
@Composable
fun CalendarPage(model: AppModel) {
    val calendar = model.calendar
    val palette = LocalPalette.current
    val tv = LocalTV.current
    val gutter = if (tv) 0.dp else 16.dp
    val page = calendar.page
    val picked = calendar.pickedSquare(page)
    LaunchedEffect(Unit) { calendar.appeared() }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(start = gutter, end = gutter, top = 12.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            PageTitle("Calendar")
            Spacer(Modifier.weight(1f))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Chip("Month", !calendar.week) { calendar.chooseWeek(false) }
                Chip("Week", calendar.week) { calendar.chooseWeek(true) }
            }
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            StepButton("back", if (calendar.week) "The week before" else "The month before") { calendar.step(-1) }
            Text(
                page.title,
                Modifier.weight(1f),
                style = Type.font(if (tv) 20.sp else 15.sp, FontWeight.Bold, palette.text),
                textAlign = TextAlign.Center,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            StepButton("forward", if (calendar.week) "The week after" else "The month after") { calendar.step(1) }
            Chip("Today", false) { calendar.step(0) }
        }
        DayGrid(page, picked?.day) { calendar.picked = it }
        if (page.note.isNotEmpty()) Text(page.note, style = Type.font(if (tv) 16.sp else 13.sp, color = palette.textDim))
        if (picked != null) {
            Text(picked.longName, Modifier.padding(top = 4.dp), style = Type.heading(if (tv) 24.sp else 18.sp, palette.text))
            if (picked.entries.isEmpty()) {
                Text("Nothing from your Watchlist comes out this day.", style = Type.font(if (tv) 16.sp else 14.sp, color = palette.textDim))
            }
            picked.entries.forEach { entry ->
                EntryRow(entry) {
                    model.open(Opening(entry.id, entry.kind, entry.title, video = entry.video.ifEmpty { null }))
                }
            }
        }
        Text(
            CalendarRules.zoneNote(model.library.zone),
            Modifier.padding(top = 6.dp),
            style = Type.font(if (tv) 14.sp else 11.sp, color = palette.textDim),
        )
    }
}

/** A round arrow to the month (or week) before or after. */
@Composable
private fun StepButton(icon: String, name: String, onClick: () -> Unit) {
    val palette = LocalPalette.current
    val size = if (LocalTV.current) 44.dp else 36.dp
    Box(
        Modifier
            .size(size)
            .background(palette.surface, CircleShape)
            .border(1.dp, palette.border, CircleShape)
            .pressable(CircleShape, onClick = onClick)
            .semantics { contentDescription = name },
        contentAlignment = Alignment.Center,
    ) {
        HavenIcon(icon, 18.dp, palette.text)
    }
}

/** The squares, seven across under the days' names. */
@Composable
private fun DayGrid(page: CalendarRules.Page, picked: Day?, pick: (Day) -> Unit) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth().clearAndSetSemantics {}, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            CalendarRules.weekdays.forEach { name ->
                Text(
                    name,
                    Modifier.weight(1f),
                    style = Type.font(if (tv) 14.sp else 11.sp, FontWeight.SemiBold, palette.textDim),
                    textAlign = TextAlign.Center,
                )
            }
        }
        page.squares.chunked(7).forEach { week ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                week.forEach { square ->
                    DaySquare(square, square.day == picked, Modifier.weight(1f)) { pick(square.day) }
                }
            }
        }
    }
}

@Composable
private fun DaySquare(square: CalendarRules.Square, picked: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    val shape = RoundedCornerShape(10.dp)
    val marked = picked || square.today
    val fill = when {
        picked -> palette.accent.copy(alpha = 0.22f)
        square.today -> palette.selected
        square.inMonth -> palette.surface
        else -> Color.Transparent
    }
    val edge = when {
        marked -> palette.accent
        square.inMonth -> palette.border
        else -> Color.Transparent
    }
    Column(
        modifier
            .heightIn(min = if (tv) 64.dp else 50.dp)
            .background(fill, shape)
            .border(if (marked) 1.5.dp else 1.dp, edge, shape)
            .pressable(shape, onClick = onClick)
            .clearAndSetSemantics {
                contentDescription = square.longName + if (square.entries.isEmpty()) "" else ", ${square.entries.size} coming out"
                selected = picked
            }
            .padding(vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
    ) {
        Text(
            "${square.day.day}",
            Modifier.alpha(if (square.inMonth) 1f else 0.5f),
            style = Type.font(
                if (tv) 18.sp else 14.sp,
                if (marked) FontWeight.Bold else FontWeight.Medium,
                if (marked) palette.accent else if (square.inMonth) palette.text else palette.textDim,
            ),
        )
        Box(Modifier.height(16.dp), contentAlignment = Alignment.Center) {
            when {
                square.allWatched -> WatchedTick(16.dp)
                square.entries.isNotEmpty() -> Row(
                    Modifier.alpha(if (square.inMonth) 1f else 0.55f),
                    horizontalArrangement = Arrangement.spacedBy(3.dp),
                ) {
                    // One dot a release, up to three.
                    repeat(minOf(square.entries.size, 3)) { Box(Modifier.size(6.dp).background(palette.accent, CircleShape)) }
                }
            }
        }
    }
}

/** One film or episode coming out that day. */
@Composable
private fun EntryRow(entry: CalendarRules.Entry, open: () -> Unit) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    val shape = RoundedCornerShape(10.dp)
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .background(palette.surface, shape)
            .border(1.dp, palette.border, shape)
            .pressable(shape, onClick = open)
            .clearAndSetSemantics {
                contentDescription = entry.title + (if (entry.label.isEmpty()) "" else ", ${entry.label}") + if (entry.watched) ", watched" else ""
            }
            .padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.width(4.dp).height(30.dp).background(palette.accent, RoundedCornerShape(2.dp)))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(entry.title, style = Type.font(if (tv) 18.sp else 14.sp, FontWeight.SemiBold, palette.text), maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(entry.line, style = Type.font(if (tv) 15.sp else 12.sp, color = palette.textDim), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (entry.time.isNotEmpty()) Text(entry.time, style = Type.font(if (tv) 16.sp else 13.sp, FontWeight.SemiBold, palette.accent))
        if (entry.watched) WatchedTick(24.dp)
    }
}
