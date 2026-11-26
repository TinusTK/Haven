# Shared iPhone and Apple TV files

The iPhone, iPad and Apple TV apps are written in Swift and live in
`iphone/`. This folder holds what they share with the release workflows:

- `fetch-libmpv.sh`: downloads libmpv and FFmpeg (checksum-pinned, LGPL
  builds) into `libmpv/`, which the Swift apps embed
  (`iphone/scripts/embed-libmpv.sh`).
- `sidestore_source.py` builds Haven's official AltStore / SideStore source
  (`https://tinustk.github.io/Haven/source.json`) from the releases; the
  workflow `.github/workflows/sidestore-source.yml` runs it and puts it on
  GitHub Pages whenever a release is published.
