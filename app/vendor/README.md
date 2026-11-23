# Patched Slint core

`i-slint-core/` is Slint's core crate, version **1.18.1**, copied from
crates.io with two small changes to touch scrolling. `app/Cargo.toml`
points Cargo at it (`[patch.crates-io]`), so every Slint crate uses it. Its
licence is Slint's own (see `i-slint-core/LICENSES`); Haven shows the
attribution Slint asks for ("Made with Slint").

## Why

A row that scrolls sideways (a shelf of posters) inside a page that
scrolls up and down: on a phone, a sideways swipe stopped partway, jumped
and did not glide, because Slint:

1. held the touch-down back from the row for 100 ms (to see whether the
   page should scroll), so the row ignored the first moves of a quick
   swipe and then jumped; and
2. gave the swipe to the page as soon as the finger had moved 8 px up or
   down, however far it had moved sideways.

## The changes (marked `// Haven patch:`)

- `window.rs`, `process_mouse_input`: a press a Flickable is holding back
  is handed on as soon as the pointer moves (as Slint already does on
  release).
- `items/flickable.rs`, `should_capture_mouse_direction`: a Flickable that
  moves along one axis only takes a drag that went mostly along that axis.

`haven.patch` is the same changes as a diff against the crates.io copy.
`app/crates/haven-app/tests/swipe.rs` checks them: it fails on stock Slint
and passes on this copy.

## Updating Slint

When the workspace moves to a newer Slint: copy that version's
`i-slint-core` from `~/.cargo/registry/src/*/i-slint-core-<version>/` over
this folder (drop `.cargo-ok` and `.cargo_vcs_info.json`), apply
`haven.patch` (`patch -p1 -d app/vendor/i-slint-core < app/vendor/haven.patch`),
fix anything that no longer applies, regenerate `haven.patch`, and run
`cargo test -p haven-app --test swipe`. If Slint has fixed both points
itself, remove the folder and the `[patch.crates-io]` lines instead.
