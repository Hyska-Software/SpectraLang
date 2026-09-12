; ─────────────────────────────────────────────────────────────────────────────
; SpectraLang Installer — Inno Setup 6
; Build: ISCC.exe /DAppVersion=X.Y.Z /DSourceDir=bin spectra.iss
; ─────────────────────────────────────────────────────────────────────────────

#define AppName       "SpectraLang"
#define AppPublisher  "SpectraLang"
#define AppURL        "https://github.com/Estevaobonatto/SpectraLang"
#define AppExeName    "spectralang.exe"

; AppVersion and SourceDir are passed via /D on the command line.
; Provide defaults so ISCC.exe can compile the script directly for testing.
#ifndef AppVersion
  #define AppVersion "0.0.1"
#endif
#ifndef SourceDir
  #define SourceDir "bin"
#endif

[Setup]
AppId={{B7E3D1A4-5C2F-4E8B-9D6A-3F1C0E2A7B9D}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
AllowNoIcons=yes
LicenseFile=
; Uninstaller
UninstallDisplayName={#AppName} {#AppVersion}
UninstallDisplayIcon={app}\spectra-icon.ico
; Output
OutputDir=Output
OutputBaseFilename=Spectra-Setup-{#AppVersion}-windows-x64
SetupIconFile=spectra-icon.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
; Architecture
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add Spectra to the system PATH (recommended)"; GroupDescription: "Additional options:"
Name: "fileassoc"; Description: "Associate .spectra files with Spectra CLI"; GroupDescription: "Additional options:"
Name: "installvsix"; Description: "Install VS Code extension (requires VS Code)"; GroupDescription: "Additional options:"

[Files]
; Core binaries
Source: "{#SourceDir}\spectralang.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\spc.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\spectra-lsp.exe"; DestDir: "{app}"; Flags: ignoreversion
; Icon used by the installer, shortcuts and Windows file association
Source: "spectra-icon.ico"; DestDir: "{app}"; Flags: ignoreversion
; VS Code extension (installed on demand)
Source: "{#SourceDir}\spectra-vscode-extension.vsix"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Start Menu
Name: "{group}\SpectraLang CLI Reference"; Filename: "{app}\{#AppExeName}"; Parameters: "--help"
Name: "{group}\Uninstall SpectraLang"; Filename: "{uninstallexe}"

[Run]
; Install VS Code extension after setup (user can deselect the task)
Filename: "{cmd}"; Parameters: "/c code --install-extension ""{app}\spectra-vscode-extension.vsix"" --force"; Flags: runhidden waituntilterminated; StatusMsg: "Installing VS Code extension..."; Description: "Install the SpectraLang VS Code extension"; Tasks: installvsix
; Open a new PowerShell so the user can try spectra-cli immediately (PATH already active)
Filename: "powershell.exe"; Parameters: "-NoExit -Command ""$env:PATH = [System.Environment]::GetEnvironmentVariable('Path','User') + ';' + [System.Environment]::GetEnvironmentVariable('Path','Machine'); Write-Host 'SpectraLang installed. Run: spectralang --help (or spc --help)' -ForegroundColor Green"""; Description: "Open a terminal to try spectralang"; Flags: postinstall skipifsilent nowait; Tasks: addtopath

[Registry]
; ── PATH management (user-level so no UAC prompt by default) ─────────────────
; Add {app} to HKCU PATH
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "SPECTRA_HOME"; ValueData: "{app}"; Flags: uninsdeletevalue; Tasks: addtopath

; ── .spectra file association ─────────────────────────────────────────────────
; File type class
Root: HKCU; Subkey: "Software\Classes\.spectra"; ValueType: string; ValueName: ""; ValueData: "SpectraLang.SourceFile"; Flags: uninsdeletevalue; Tasks: fileassoc
Root: HKCU; Subkey: "Software\Classes\SpectraLang.SourceFile"; ValueType: string; ValueName: ""; ValueData: "Spectra Source File"; Flags: uninsdeletekey; Tasks: fileassoc
Root: HKCU; Subkey: "Software\Classes\SpectraLang.SourceFile\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\spectra-icon.ico"; Tasks: fileassoc
Root: HKCU; Subkey: "Software\Classes\SpectraLang.SourceFile\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"" run ""%1"""; Tasks: fileassoc

; ── Notify shell of registry changes ──────────────────────────────────────────
Root: HKCU; Subkey: "Software\Classes\.spectra"; ValueType: string; ValueName: ""; ValueData: "SpectraLang.SourceFile"; Flags: uninsdeletevalue; Tasks: fileassoc

[Code]
// ─────────────────────────────────────────────────────────────────────────────
// PATH management: append/remove {app} from HKCU\Environment\Path
// ─────────────────────────────────────────────────────────────────────────────
const
  EnvKey     = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';
  UserEnvKey = 'Environment';

// Broadcast environment change so new terminals pick up the updated PATH
function SendMessageTimeoutA(hWnd: DWORD; Msg: UINT; wParam: UINT; lParam: AnsiString; fuFlags: UINT; uTimeout: UINT; var lpdwResult: DWORD): DWORD;
  external 'SendMessageTimeoutA@user32.dll stdcall';

procedure AddToUserPath(Dir: string);
var
  OldPath, NewPath: string;
  ResultCode: DWORD;
begin
  if not RegQueryStringValue(HKCU, UserEnvKey, 'Path', OldPath) then
    OldPath := '';

  if Pos(LowerCase(Dir), LowerCase(OldPath)) = 0 then
  begin
    if (Length(OldPath) > 0) and (OldPath[Length(OldPath)] <> ';') then
      NewPath := OldPath + ';' + Dir
    else
      NewPath := OldPath + Dir;
    RegWriteStringValue(HKCU, UserEnvKey, 'Path', NewPath);
    // Properly notify the system so new terminals see the updated PATH
    SendMessageTimeoutA(HWND_BROADCAST, $001A, 0, 'Environment', $0002, 5000, ResultCode);
  end;
end;

procedure RemoveFromUserPath(Dir: string);
var
  OldPath, NewPath: string;
  Parts: TStringList;
  I: Integer;
begin
  if not RegQueryStringValue(HKCU, UserEnvKey, 'Path', OldPath) then
    Exit;

  Parts := TStringList.Create;
  try
    Parts.Delimiter := ';';
    Parts.DelimitedText := OldPath;
    NewPath := '';
    for I := 0 to Parts.Count - 1 do
    begin
      if LowerCase(Trim(Parts[I])) <> LowerCase(Dir) then
      begin
        if NewPath <> '' then
          NewPath := NewPath + ';';
        NewPath := NewPath + Parts[I];
      end;
    end;
    RegWriteStringValue(HKCU, UserEnvKey, 'Path', NewPath);
  finally
    Parts.Free;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    if IsTaskSelected('addtopath') then
      AddToUserPath(ExpandConstant('{app}'));
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromUserPath(ExpandConstant('{app}'));
end;
