# Haven for iOS

- `bundle.sh`: builds Haven for iPhone and iPad and makes `Haven.app` and
  `Haven-<version>-ios-unsigned.ipa` in `app/target/ios`;
  `bundle.sh simulator` makes an app for the iOS Simulator instead.
- `Info.plist`: the app's details; its identifier is `net.haven.app`.

The IPA is not signed. To put it on a device, whoever installs it signs it
with their own Apple ID, with a tool such as AltStore or Sideloadly. A
signed build for App Store Connect is planned for later.

On an iPhone the pages are laid out for a narrow screen with tabs along
the bottom (an iPad gets the layout a computer gets), there is no Add-ons
page, and videos open in a player app chosen in Settings: VLC, Infuse or
Outplayer.
