; Inno Setup script for the vibez installer.
; Built in CI with: ISCC /DMyAppVersion=<version> vibez.iss

#ifndef MyAppVersion
#define MyAppVersion "0.0.0"
#endif

[Setup]
AppId={{8F2C1B6A-4E1D-4C6B-9A57-3D2E9C4B7F10}
AppName=vibez
AppVersion={#MyAppVersion}
AppPublisher=Alex Wanyoike
AppPublisherURL=https://github.com/alexanderwanyoike/vibez
DefaultDirName={autopf}\vibez
DefaultGroupName=vibez
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputBaseFilename=vibez-setup
SetupIconFile=..\..\assets\icon\vibez.ico
UninstallDisplayIcon={app}\vibez.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop icon"; GroupDescription: "Additional icons:"; Flags: unchecked

[Files]
Source: "..\..\target\release\vibez.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\vibez-plugin-scan.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\demo.vibez"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\vibez"; Filename: "{app}\vibez.exe"
Name: "{autodesktop}\vibez"; Filename: "{app}\vibez.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\vibez.exe"; Description: "Launch vibez"; Flags: nowait postinstall skipifsilent
