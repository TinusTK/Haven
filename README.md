# Haven

A media hub for films, series, live TV and your own media servers, written
in Rust with Slint. One codebase builds every version.

| Folder | What is in it |
| --- | --- |
| `app/` | The code all versions share: the Cargo workspace, the UI, the icons and fonts. |
| `windows/` | The Windows installer script and what fetches the video library for it. |
| `mac/` | What makes `Haven.app` and its disk image, and fetches the video library for it. |
| `linux/` | What the `.deb` package needs beside the program. |
| `android/` | The Android app's resources; the APK is built from `app/`. |
| `ios/` | The iOS version (not started yet). |
| `licenses/` | Licence texts of what ships with Haven; see `THIRD-PARTY.md`. |

Each platform's folder has a README saying how that version is built.
Downloads are on the Releases page.
