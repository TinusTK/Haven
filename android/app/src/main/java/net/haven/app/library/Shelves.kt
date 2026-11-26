package net.haven.app.library

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import net.haven.app.metadata.TmdbList
import net.haven.app.pictures.Generation
import net.haven.app.servers.Server
import net.haven.app.titles.Catalog
import net.haven.app.titles.MetaPreview

/** The places that show shelves. */
enum class Board(val raw: String) {
    /** The catalogues' shelves on Home (Cinemeta, add-ons). */
    Home("Home"),
    Search("Search"),
    /** The media servers' shelves on Home. */
    Servers("Servers"),
    /** The TMDB catalogue's shelves on Home. */
    Catalog("Catalog"),
}

/** A row of posters with a heading. */
data class Shelf(
    /** Its place on the board. */
    val id: Int,
    val title: String,
    /** Where its titles come from ("Cinemeta", a server's name, "TMDB"). */
    val source: String,
    val loading: Boolean = true,
    val error: String = "",
    val items: List<MetaPreview> = emptyList(),
    /** The name it is kept under for the next start; empty: not kept. */
    val keptName: String = "",
)

/** Where a shelf's titles come from. */
sealed class ShelfFrom {
    /** A catalogue of a source (Cinemeta or an add-on), by the source's address. */
    data class Catalogue(val url: String, val catalog: Catalog) : ShelfFrom()
    /** A library on a media server: its newest titles, or all by name. */
    data class OnServer(val server: Server, val library: String, val latest: Boolean) : ShelfFrom()
    /** A media server's answer to the search words. */
    data class ServerSearch(val server: Server) : ShelfFrom()
    /** One of TMDB's ready-made lists of films or series. */
    data class TmdbShelf(val list: TmdbList, val series: Boolean) : ShelfFrom()
    /** TMDB's answer to the search words. */
    data class TmdbSearch(val series: Boolean) : ShelfFrom()
}

/** A shelf to build. */
data class ShelfPlan(val title: String, val source: String, val from: ShelfFrom)

/** One board's shelves. Rebuilding it starts a new generation, so late
 *  answers for the old shelves are dropped. */
class ShelfBoard(val board: Board) {
    var shelves by mutableStateOf(listOf<Shelf>())
    val generation = Generation()

    /** How many shelves have answered (for the loading screen). */
    val settled get() = shelves.count { !it.loading }
}

/** What the shelves show: the start of a list (Explore shows it all). */
object ShelfLength {
    const val SHELF = 30
    /** A media server's library shelf. */
    const val SERVER = 60
    /** Media servers' shelves fetched at once (a server that gathers
     *  catalogues refuses bursts). */
    const val SERVERS_AT_ONCE = 2
}

/** The TMDB shelves on Home, in order (only with a TMDB key). */
object TmdbShelves {
    data class Plan(val title: String, val list: TmdbList, val series: Boolean)

    val plans = listOf(
        Plan("Trending films", TmdbList.Trending, false),
        Plan("Trending series", TmdbList.Trending, true),
        Plan("In cinemas now", TmdbList.Now, false),
        Plan("On television now", TmdbList.Now, true),
        Plan("Popular films", TmdbList.Popular, false),
        Plan("Popular series", TmdbList.Popular, true),
        Plan("Coming soon", TmdbList.Upcoming, false),
        Plan("Top rated films", TmdbList.TopRated, false),
        Plan("Top rated series", TmdbList.TopRated, true),
    )
}
