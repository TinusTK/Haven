# Haven for iPhone (Swift)

Haven's iPhone app written in Swift and SwiftUI. It is being built to
replace the Rust + Slint iPhone build, and does the same things. It is the
same app (`net.haven.app`), and it keeps the same data, so an update keeps
everything.

## Building

You need Xcode, [XcodeGen](https://github.com/yonaskolb/XcodeGen)
(`brew install xcodegen`) and the GitHub CLI (to fetch libmpv the first
time).

```bash
iphone/scripts/build.sh github simulator   # GitHub build in the simulator
iphone/scripts/build.sh store simulator    # App Store build (+ store check)
iphone/scripts/build.sh github test        # unit tests
iphone/scripts/build.sh store device       # unsigned build for an iPhone
```

The Xcode project is made from `project.yml` each time (open
`iphone/Haven.xcodeproj` after a build to work in Xcode). Build output goes
to `iphone/build`.

## Two kinds of build

- **GitHub** (scheme `Haven`, flag `ADDONS`): add-ons, media servers at
  any address.
- **App Store / TestFlight** (scheme `HavenStore`, flag `STORE`): no
  add-ons, media servers on the Local Area Network only. Its backup goes
  to iCloud Drive › Haven. `scripts/store-check.sh` fails the build if the
  app has any add-on code or wording.

## Keys

The Trakt, Simkl and OMDb app keys are built in from `HAVEN_*` variables
(`scripts/build-keys.sh`). There is no built-in TMDB key: people add their
own in Settings.

## Video

Videos play through libmpv, the same frameworks the Rust build uses,
fetched by `ios/fetch-libmpv.sh` and shipped as separate frameworks in
`Haven.app/Frameworks`.
