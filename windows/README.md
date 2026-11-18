# Haven for Windows

- `haven.iss`: the installer (Inno Setup 6). CI runs
  `iscc /DAppVersion=<version> windows\haven.iss`; the installer lands in
  `app\target\installer`.
- `fetch-libmpv.ps1`: fills `libmpv\` with the LGPL video library
  (`libmpv-2.dll`), `mpv.lib` and the Vulkan loader. See `libmpv\README.md`.

To build by hand: run `fetch-libmpv.ps1`, then in `app\`
`cargo build --release -p haven-app --features player`, then the `iscc`
line above.
