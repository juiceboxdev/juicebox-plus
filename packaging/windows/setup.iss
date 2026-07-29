[Setup]
AppName=juicebox-plus
AppVersion={#AppVersion}
AppVerName=juicebox-plus {#AppVersion}
AppPublisher=JuiceyDev
AppPublisherURL=https://github.com/juiceboxdev/juicebox-plus
DefaultDirName={pf}\juicebox-plus
DefaultGroupName=juicebox-plus
OutputDir=.
OutputBaseFilename=juicebox-plus-setup-{#AppVersion}
Compression=lzma
SolidCompression=yes
UninstallDisplayIcon={app}\juicebox-plus.exe
DisableProgramGroupPage=yes

[Files]
Source: "juicebox-plus.exe"; DestDir: "{app}"

[Icons]
Name: "{autoprograms}\juicebox-plus"; Filename: "{app}\juicebox-plus.exe"; WorkingDir: "{app}"
Name: "{autoprograms}\Uninstall juicebox-plus"; Filename: "{uninstallexe}"
Name: "{autodesktop}\juicebox-plus"; Filename: "{app}\juicebox-plus.exe"; WorkingDir: "{app}"
