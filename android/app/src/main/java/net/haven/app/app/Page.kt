package net.haven.app.app

/** Haven's pages, numbered as the Rust build saved them (`last_page`). */
enum class Page(val label: String, val icon: String) {
    Home("Home", "home"),
    Explore("Explore", "explore"),
    Library("Library", "library"),
    LiveTV("Live TV", "livetv"),
    Calendar("Calendar", "calendar"),
    Settings("Settings", "settings");

    companion object {
        /** The page saved as `index`; Home when it is out of range. */
        fun of(index: Int): Page = entries.getOrNull(index) ?: Home
    }
}

/** How the pages are laid out: a phone has the tab bar along the bottom; a
 *  tablet (or any wide window) the sidebar down the left; a TV the tabs
 *  along the top, moved through with the remote. */
enum class Layout {
    Phone, Tablet, TV;

    companion object {
        /** From the screen's width in dp: 600 and up is a tablet. */
        fun of(tv: Boolean, widthDp: Float): Layout = when {
            tv -> TV
            widthDp >= 600f -> Tablet
            else -> Phone
        }
    }
}
