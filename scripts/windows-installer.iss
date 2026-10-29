; Compiled by package-windows.py. Payload is an explicit, audited staging tree.
#ifndef Payload
  #error Payload is required
#endif
#ifndef AppVersion
  #error AppVersion is required
#endif
[Setup]
AppId={{D891B420-751F-4627-ACD6-56BAA778CE81}
AppName=TrueRenderer
AppVersion={#AppVersion}
AppPublisher=TrueRenderer
DefaultDirName={localappdata}\Programs\TrueRenderer
DefaultGroupName=TrueRenderer
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0
OutputDir={#Output}
OutputBaseFilename=TrueRenderer-{#AppVersion}-Setup-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
LicenseFile={#Payload}\LICENSE
UninstallDisplayIcon={app}\truerenderer.exe
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes
SetupLogging=yes

[Languages]
Name: "italian"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "fileassociation"; Description: "Associa i documenti .truerenderer"; Flags: unchecked

[Files]
Source: "{#Payload}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\TrueRenderer"; Filename: "{app}\truerenderer.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\TrueRenderer"; Filename: "{app}\truerenderer.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\.truerenderer\OpenWithProgids"; ValueType: string; ValueName: "TrueRenderer.InstalledDocument"; ValueData: ""; Flags: uninsdeletevalue; Tasks: fileassociation
Root: HKCU; Subkey: "Software\Classes\TrueRenderer.InstalledDocument"; ValueType: string; ValueData: "Documento TrueRenderer"; Flags: uninsdeletekey; Tasks: fileassociation
Root: HKCU; Subkey: "Software\Classes\TrueRenderer.InstalledDocument\DefaultIcon"; ValueType: string; ValueData: """{app}\truerenderer.exe"",0"; Tasks: fileassociation
Root: HKCU; Subkey: "Software\Classes\TrueRenderer.InstalledDocument\shell\open\command"; ValueType: string; ValueData: """{app}\truerenderer.exe"" --open ""%1"""; Tasks: fileassociation

[Run]
Filename: "{app}\truerenderer.exe"; Description: "{cm:LaunchProgram,TrueRenderer}"; Flags: nowait postinstall skipifsilent

; No UninstallDelete: user-created files and the separate LocalAppData catalog
; are intentionally retained, including when uninstalling before reinstalling.
