# Haven for Linux

- `haven.desktop`: the menu entry the `.deb` installs.

The package is made in `app/` with
`cargo deb -p haven-app --features player` (it needs `libmpv-dev` to build
and uses the system's libmpv to run). What goes into it is listed under
`[package.metadata.deb]` in `app/crates/haven-app/Cargo.toml`.
