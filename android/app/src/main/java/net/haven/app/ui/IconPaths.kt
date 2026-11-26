package net.haven.app.ui

/** Haven's own icons: the Rust build's line drawings, as SVG path text in a
 *  24×24 box (drawn with a 2 dp line; "play" is filled). Copied from the
 *  Swift build's `IconPaths`. */
object IconPaths {
    val drawings: Map<String, String> = mapOf(
        "home" to "M3 10.5 L12 3 L21 10.5 M5.5 9 V20 a1 1 0 0 0 1 1 H10 V15 H14 V21 H17.5 a1 1 0 0 0 1 -1 V9",
        "explore" to "M12 2.5 a9.5 9.5 0 1 0 0 19 a9.5 9.5 0 1 0 0 -19 Z M15.8 8.2 L13.6 13.6 L8.2 15.8 L10.4 10.4 Z",
        "library" to "M4 4 H8 V20 H4 Z M10 4 H14 V20 H10 Z M15.6 5.2 L19.4 4.2 L22 19 L18.2 20 Z",
        "livetv" to "M3 7 H21 a1 1 0 0 1 1 1 V18 a1 1 0 0 1 -1 1 H3 a1 1 0 0 1 -1 -1 V8 a1 1 0 0 1 1 -1 Z M8 3 L12 7 L16 3",
        "calendar" to "M4 5 H20 a1 1 0 0 1 1 1 V20 a1 1 0 0 1 -1 1 H4 a1 1 0 0 1 -1 -1 V6 a1 1 0 0 1 1 -1 Z M3 10 H21 M8 3 V7 M16 3 V7",
        "search" to "M10.5 3.5 a7 7 0 1 0 0 14 a7 7 0 1 0 0 -14 Z M15.5 15.5 L21 21",
        "settings" to "M18.88 9.87 L21.66 10.34 L21.66 13.66 L18.88 14.13 L18.37 15.36 L20.00 17.66 L17.66 20.00 L15.36 18.37 L14.13 18.88 L13.66 21.66 L10.34 21.66 L9.87 18.88 L8.64 18.37 L6.34 20.00 L4.00 17.66 L5.63 15.36 L5.12 14.13 L2.34 13.66 L2.34 10.34 L5.12 9.87 L5.63 8.64 L4.00 6.34 L6.34 4.00 L8.64 5.63 L9.87 5.12 L10.34 2.34 L13.66 2.34 L14.13 5.12 L15.36 5.63 L17.66 4.00 L20.00 6.34 L18.37 8.64 Z M12 9 a3 3 0 1 0 0 6 a3 3 0 1 0 0 -6 Z",
        "download" to "M12 3 V15 M7 10 L12 15 L17 10 M4 20 H20",
        "trash" to "M4 7 H20 M9 7 V4 H15 V7 M6 7 L7 21 H17 L18 7",
        "play" to "M7 4 L20 12 L7 20 Z",
        "pause" to "M8 5 V19 M16 5 V19",
        "folder" to "M3 6 a1 1 0 0 1 1 -1 H9 L11 7 H20 a1 1 0 0 1 1 1 V18 a1 1 0 0 1 -1 1 H4 a1 1 0 0 1 -1 -1 Z",
        "check" to "M5 12.5 L10 17.5 L19 7",
        "close" to "M6 6 L18 18 M18 6 L6 18",
        "back" to "M15 5 L8 12 L15 19",
        "forward" to "M9 5 L16 12 L9 19",
        "radio" to "M4 9 H20 a1 1 0 0 1 1 1 V19 a1 1 0 0 1 -1 1 H4 a1 1 0 0 1 -1 -1 V10 a1 1 0 0 1 1 -1 Z M6 9 L17 4 M15.5 12.5 a2.5 2.5 0 1 0 0 5 a2.5 2.5 0 1 0 0 -5 Z M6 13 H11 M6 16 H11",
        "account" to "M12 4 a4 4 0 1 0 0 8 a4 4 0 1 0 0 -8 Z M4 21 a8 8 0 0 1 16 0",
        "server" to "M4 4 H20 V10 H4 Z M4 14 H20 V20 H4 Z M7 7 H8 M7 17 H8",
        "addon" to "M5 8 H9 a2 2 0 1 1 4 0 H17 V12 a2 2 0 1 1 0 4 V20 H5 Z",
        "info" to "M12 3 a9 9 0 1 0 0 18 a9 9 0 1 0 0 -18 Z M12 11 V16 M12 7.5 V8",
        "general" to "M4 6 H20 M4 12 H20 M4 18 H20 M9 4 V8 M15 10 V14 M7 16 V20",
        "storage" to "M4 5 H20 V19 H4 Z M4 14 H20 M7 16.5 H8",
        "paste" to "M8 5 H6 a1 1 0 0 0 -1 1 V20 a1 1 0 0 0 1 1 H18 a1 1 0 0 0 1 -1 V6 a1 1 0 0 0 -1 -1 H16 M9 3 H15 V7 H9 Z M9 12 H15 M9 16 H13",
        "sync" to "M20 11 a8 8 0 0 0 -14.5 -4.5 M4 4 V8 H8 M4 13 a8 8 0 0 0 14.5 4.5 M20 20 V16 H16",
        "plus" to "M12 5 V19 M5 12 H19",
        "link" to "M10 14 L14 10 M8.5 11.5 L6.5 13.5 a3 3 0 0 0 4 4 L12.5 15.5 M15.5 12.5 L17.5 10.5 a3 3 0 0 0 -4 -4 L11.5 8.5",
        "player" to "M3 5 H21 V19 H3 Z M10 9 L15 12 L10 15 Z",
    )
}
