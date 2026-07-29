[Setup]
AppName=juicebox-plus
AppVersion={#AppVersion}
AppPublisher=JuiceyDev
AppPublisherURL=https://github.com/juiceboxdev/juicebox-plus
DefaultDirName={pf}\juicebox-plus
DefaultGroupName=juicebox-plus
OutputDir=.
OutputBaseFilename=juicebox-plus-setup-{#AppVersion}
Compression=lzma
SolidCompression=yes
UninstallDisplayIcon={app}\juicebox-plus.exe

[Files]
Source: "juicebox-plus.exe"; DestDir: "{app}"

[Icons]
Name: "{group}\juicebox-plus"; Filename: "{app}\juicebox-plus.exe"
Name: "{group}\Uninstall juicebox-plus"; Filename: "{uninstallexe}"
