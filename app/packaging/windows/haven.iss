; Haven installer for Windows (Inno Setup 6).
; Built by CI: iscc /DAppVersion=<version> packaging\windows\haven.iss
; Expects haven.exe in target\release, and, when the player was built in,
; libmpv-2.dll (an LGPL build) in packaging\windows\libmpv.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
; Never change the AppId: it is how Windows knows a newer Haven replaces an older one.
AppId={{6B0E3B0C-6F0D-4C0E-9B7C-3F3D0A7F7C21}
AppName=Haven
AppVersion={#AppVersion}
AppPublisher=TinusTK
AppCopyright=Copyright (c) 2026 TinusTK. All rights reserved.
DefaultDirName={autopf}\Haven
DefaultGroupName=Haven
DisableProgramGroupPage=yes
LicenseFile=..\..\..\LICENSE
SetupIconFile=..\..\assets\icon\haven.ico
UninstallDisplayIcon={app}\haven.exe
OutputDir=..\..\target\installer
OutputBaseFilename=Haven-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\haven.exe"; DestDir: "{app}"; Flags: ignoreversion
; The video engine and whatever it needs beside it; absent in a build without the player.
Source: "libmpv\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\..\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Haven"; Filename: "{app}\haven.exe"
Name: "{autodesktop}\Haven"; Filename: "{app}\haven.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\haven.exe"; Description: "{cm:LaunchProgram,Haven}"; Flags: nowait postinstall skipifsilent
