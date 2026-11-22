; Haven installer for Windows (Inno Setup 6).
; Built by CI: iscc /DAppVersion=<version> windows\haven.iss
; Expects haven.exe in app\target\release, and, when the player was built in,
; libmpv-2.dll (an LGPL build) in windows\libmpv.

#ifndef AppVersion
  #define AppVersion "0.2.1"
#endif

[Setup]
; Never change the AppId: it is how Windows knows a newer Haven replaces an older one.
AppId={{6B0E3B0C-6F0D-4C0E-9B7C-3F3D0A7F7C21}
AppName=Haven
AppVersion={#AppVersion}
AppPublisher=TinusTK
AppCopyright=Copyright (c) 2026 TinusTK. Licensed under the GNU GPL version 3.
DefaultDirName={autopf}\Haven
DefaultGroupName=Haven
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
SetupIconFile=..\app\assets\icon\haven.ico
UninstallDisplayIcon={app}\haven.exe
OutputDir=..\app\target\installer
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
Source: "..\app\target\release\haven.exe"; DestDir: "{app}"; Flags: ignoreversion
; The video engine and whatever it needs beside it; absent in a build without the player.
Source: "libmpv\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "libmpv\VulkanRT-License.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion
; Licence texts of the libraries shipped beside haven.exe (see THIRD-PARTY.md).
Source: "..\licenses\*.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{group}\Haven"; Filename: "{app}\haven.exe"
Name: "{autodesktop}\Haven"; Filename: "{app}\haven.exe"; Tasks: desktopicon

[Run]
; Not skipped in a silent install: that is how Haven starts again after
; it has updated itself (it runs this installer with /SILENT).
Filename: "{app}\haven.exe"; Description: "{cm:LaunchProgram,Haven}"; Flags: nowait postinstall
