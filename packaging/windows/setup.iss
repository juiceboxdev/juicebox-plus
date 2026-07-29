[Setup]
AppName=Juicebox Plus
AppVersion={#AppVersion}
AppVerName=Juicebox Plus {#AppVersion}
AppPublisher=JuiceyDev
AppPublisherURL=https://github.com/juiceboxdev/juicebox-plus
DefaultDirName={pf}\Juicebox Plus
DefaultGroupName=Juicebox Plus
OutputDir=.
OutputBaseFilename=juicebox-plus-setup-{#AppVersion}
Compression=lzma
SolidCompression=yes
UninstallDisplayIcon={app}\juicebox-plus.exe
DisableProgramGroupPage=yes
SetupIconFile=setup.ico
UninstallDisplayName=Juicebox Plus {#AppVersion}

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "pt_BR"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "nl"; MessagesFile: "compiler:Languages\Dutch.isl"
Name: "pl"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: startup; Description: "Launch Juicebox Plus on startup"; GroupDescription: "Startup options:"

[Files]
Source: "juicebox-plus.exe"; DestDir: "{app}"
Source: "setup.ico"; DestDir: "{app}"

[Icons]
Name: "{autoprograms}\Juicebox Plus"; Filename: "{app}\juicebox-plus.exe"; WorkingDir: "{app}"
Name: "{autoprograms}\Uninstall Juicebox Plus"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Juicebox Plus"; Filename: "{app}\juicebox-plus.exe"; WorkingDir: "{app}"
Name: "{userstartup}\Juicebox Plus"; Filename: "{app}\juicebox-plus.exe"; WorkingDir: "{app}"; Tasks: startup

[Run]
Filename: "{app}\juicebox-plus.exe"; Description: "Launch Juicebox Plus"; Flags: postinstall nowait skipifsilent unchecked
