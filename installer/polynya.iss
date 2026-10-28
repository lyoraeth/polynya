; Copyright (c) Danil Klimov (lyoraeth)
; SPDX-License-Identifier: GPL-3.0-or-later
;
; Polynya installer (Inno Setup 6.5+). Built by installer/build.ps1:
;   ISCC /DArch=x64|arm64 /DBin=<folder with the built exes> /DEverything=<Everything setup.exe> polynya.iss
;
; Installs per user, without admin rights, into %LOCALAPPDATA%\Polynya: the core expects its exe
; exactly there (src/install.rs) and would otherwise move itself. Only the picked modules are
; copied. An update refreshes the modules already there and can add new ones; it removes none,
; since agents and notify leave hooks and a package behind (a module is turned off in the
; settings). Autostart and the interface language are applied on the first install only, so an
; update keeps what the user changed since.

#ifndef Arch
  #define Arch "x64"
#endif
#ifndef Bin
  #define Bin "..\target\release"
#endif
#ifndef Everything
  #error Pass /DEverything=<path to the Everything x64 setup.exe>
#endif

#define Core Bin + "\polynya.exe"
#define AppVersion GetStringFileInfo(Core, "ProductVersion")

[Setup]
AppId={{793DD86B-4DA6-450E-B1F6-D7D1CF02CBB4}
AppName=Polynya
AppVersion={#AppVersion}
AppVerName=Polynya {#AppVersion}
AppPublisher=lyoraeth
AppPublisherURL=https://github.com/lyoraeth/polynya
AppSupportURL=https://github.com/lyoraeth/polynya/issues
AppUpdatesURL=https://github.com/lyoraeth/polynya/releases
VersionInfoVersion={#GetVersionNumbersString(Core)}
DefaultDirName={localappdata}\Polynya
DisableDirPage=yes
DisableWelcomePage=no
DisableProgramGroupPage=yes
DisableReadyPage=yes
; Module components follow what is on disk, not the previous run (InitializeWizard); this also
; covers the component picks.
UsePreviousSetupType=no
PrivilegesRequired=lowest
MinVersion=10.0.22000
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
; Also runs on ARM64 under emulation, so an ARM64 user who picked the wrong file is not stuck.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
OutputDir=..\target\installer
OutputBaseFilename=Polynya-{#AppVersion}-{#Arch}-setup
SetupIconFile=..\assets\logo\polynya.ico
UninstallDisplayIcon={app}\polynya.exe
UninstallDisplayName=Polynya
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; The island is closed by `polynya quit` (PrepareToInstall); this only catches what is left.
CloseApplications=yes
RestartApplications=no
ShowLanguageDialog=yes
; With the guard on, the processes Setup starts cannot follow symlinks either (checked on 6.7,
; despite the docs): the island launched at the end would not find node behind nvm's symlink, nor
; would the apps its launcher starts. The guard protects an elevated setup from planted links; this
; one runs with the user's own rights, so a link gains an attacker nothing.
RedirectionGuard=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "zh"; MessagesFile: "ChineseSimplified.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[Messages]
; Stock texts that wrap mid-word in Russian at this width (Inno 6.7), shortened in every language.
; ReadyLabel2a: there is nowhere to go "Back" to on an update, where the Ready page shows up.
en.ReadyLabel2a=Click Install to continue.
ru.ReadyLabel2a=Нажмите «Установить», чтобы продолжить.
zh.ReadyLabel2a=单击“安装”继续。
ja.ReadyLabel2a=続行するには［インストール］をクリックしてください。
en.ExitSetupMessage=Setup is not complete. Exit Setup?
ru.ExitSetupMessage=Установка не завершена. Выйти?
zh.ExitSetupMessage=安装尚未完成。要退出安装程序吗？
ja.ExitSetupMessage=セットアップは完了していません。終了しますか？
en.WelcomeLabel2=This will install Polynya, a living taskbar and a fast launcher for Windows 11.
ru.WelcomeLabel2=Будет установлена Polynya — живая панель задач и быстрый лаунчер для Windows 11.
zh.WelcomeLabel2=将安装 Polynya：适用于 Windows 11 的动态任务栏和快速启动器。
ja.WelcomeLabel2=Polynya をインストールします。Windows 11 のための生きたタスクバーと高速ランチャーです。

[CustomMessages]
; Component names stay short; the notes go to the grey column on the right (CurPageChanged).
en.Modules=Modules
en.ModAgents=AI agents
en.ModPicker=Color picker
en.ModNotify=Windows notifications
en.Everything=Everything — file search
en.NoteAgents=Claude Code, Codex, Cursor
en.NoteNotify=experimental
en.NoteAdmin=requires admin rights
en.AlreadyInstalled=already installed
en.Autostart=Start Polynya when I sign in to Windows
en.UpdateTo=Polynya will be updated to version %1.
en.Purge=Also delete the Polynya settings and logs?
en.EverythingFailed=Everything was not installed (code %1). File search will work once Everything is installed from voidtools.com.

ru.Modules=Модули
ru.ModAgents=ИИ-агенты
ru.ModPicker=Пипетка
ru.ModNotify=Уведомления Windows
ru.Everything=Everything — поиск файлов
ru.NoteAgents=Claude Code, Codex, Cursor
ru.NoteNotify=экспериментально
ru.NoteAdmin=требуются права администратора
ru.AlreadyInstalled=уже установлен
ru.Autostart=Запускать Polynya при входе в Windows
ru.UpdateTo=Polynya будет обновлена до версии %1.
ru.Purge=Удалить также настройки и логи Polynya?
ru.EverythingFailed=Everything не установлен (код %1). Поиск файлов заработает, когда Everything будет установлен с voidtools.com.

zh.Modules=模块
zh.ModAgents=AI 代理
zh.ModPicker=取色器
zh.ModNotify=Windows 通知
zh.Everything=Everything — 文件搜索
zh.NoteAgents=Claude Code、Codex、Cursor
zh.NoteNotify=实验性
zh.NoteAdmin=需要管理员权限
zh.AlreadyInstalled=已安装
zh.Autostart=登录 Windows 时启动 Polynya
zh.UpdateTo=Polynya 将更新到版本 %1。
zh.Purge=同时删除 Polynya 的设置和日志？
zh.EverythingFailed=Everything 未安装（代码 %1）。从 voidtools.com 安装 Everything 后即可使用文件搜索。

ja.Modules=モジュール
ja.ModAgents=AI エージェント
ja.ModPicker=カラーピッカー
ja.ModNotify=Windows 通知
ja.Everything=Everything — ファイル検索
ja.NoteAgents=Claude Code、Codex、Cursor
ja.NoteNotify=試験的
ja.NoteAdmin=管理者権限が必要
ja.AlreadyInstalled=インストール済み
ja.Autostart=Windows へのサインイン時に Polynya を起動する
ja.UpdateTo=Polynya はバージョン %1 に更新されます。
ja.Purge=Polynya の設定とログも削除しますか？
ja.EverythingFailed=Everything はインストールされませんでした（コード %1）。voidtools.com から Everything をインストールすると、ファイル検索が使えるようになります。

[Types]
Name: "custom"; Description: "Custom"; Flags: iscustom

[Components]
Name: "core"; Description: "Polynya"; Types: custom; Flags: fixed
Name: "modules"; Description: "{cm:Modules}"; Types: custom
Name: "modules\agents"; Description: "{cm:ModAgents}"; Types: custom
Name: "modules\picker"; Description: "{cm:ModPicker}"; Types: custom
Name: "modules\notify"; Description: "{cm:ModNotify}"
Name: "everything"; Description: "{cm:Everything}"; Types: custom

[Tasks]
Name: "autostart"; Description: "{cm:Autostart}"; Check: IsFirstInstall

[Files]
Source: "{#Core}"; DestDir: "{app}"; Flags: ignoreversion
; The GPL travels with the binaries.
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#Bin}\polynya-agents.exe"; DestDir: "{app}\modules\agents"; Components: modules\agents; Flags: ignoreversion
Source: "..\modules\agents\module.toml"; DestDir: "{app}\modules\agents"; Components: modules\agents; Flags: ignoreversion
Source: "{#Bin}\polynya-picker.exe"; DestDir: "{app}\modules\picker"; Components: modules\picker; Flags: ignoreversion
Source: "..\modules\picker\module.toml"; DestDir: "{app}\modules\picker"; Components: modules\picker; Flags: ignoreversion
Source: "{#Bin}\polynya-notify.exe"; DestDir: "{app}\modules\notify"; Components: modules\notify; Flags: ignoreversion
Source: "..\modules\notify\module.toml"; DestDir: "{app}\modules\notify"; Components: modules\notify; Flags: ignoreversion
Source: "{#Everything}"; DestDir: "{tmp}"; DestName: "everything-setup.exe"; Components: everything; Check: WantEverything; Flags: deleteafterinstall

[Registry]
; The same value the tray toggle writes (src/autostart.rs), so the toggle shows it as on.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Polynya"; ValueData: """{app}\polynya.exe"""; Tasks: autostart

[UninstallDelete]
; Exes set aside while they were running during an update.
Type: files; Name: "{app}\*.old"
Type: files; Name: "{app}\modules\*\*.old"
; Folders that existed before this installer (laid out by the core itself) are not removed by
; Inno on its own; only when empty, so third-party modules and kept logs stay.
Type: dirifempty; Name: "{app}\modules\agents"
Type: dirifempty; Name: "{app}\modules\picker"
Type: dirifempty; Name: "{app}\modules\notify"
Type: dirifempty; Name: "{app}\modules"
Type: dirifempty; Name: "{app}"

[Code]
const
  ModuleCount = 3;

var
  FirstInstall: Boolean;
  Purge: Boolean;
  { Modules found on disk before this run, by index in ModuleId. }
  Installed: array[0..2] of Boolean;

function CoreExe(): String;
begin
  Result := ExpandConstant('{localappdata}\Polynya\polynya.exe');
end;

{ An island that is already in place counts as installed, with or without this installer: its
  config holds the user's choices, so the defaults below must not overwrite them. }
function IsFirstInstall(): Boolean;
begin
  Result := FirstInstall;
end;

function ModuleId(I: Integer): String;
begin
  case I of
    0: Result := 'agents';
    1: Result := 'picker';
  else
    Result := 'notify';
  end;
end;

{ The component caption, to find the module in the list. }
function ModuleCaption(I: Integer): String;
begin
  case I of
    0: Result := CustomMessage('ModAgents');
    1: Result := CustomMessage('ModPicker');
  else
    Result := CustomMessage('ModNotify');
  end;
end;

{ A module picked in this run that was not there before. }
function Added(I: Integer): Boolean;
begin
  Result := WizardIsComponentSelected('modules\' + ModuleId(I)) and not Installed[I];
end;

function AllModulesInstalled(): Boolean;
var
  I: Integer;
begin
  Result := True;
  for I := 0 to ModuleCount - 1 do
    Result := Result and Installed[I];
end;

function EverythingInstalled(): Boolean;
begin
  Result :=
    RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\Everything') or
    RegKeyExists(HKLM64, 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Everything') or
    RegKeyExists(HKLM32, 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Everything') or
    RegKeyExists(HKCU, 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Everything') or
    FileExists(ExpandConstant('{commonpf64}\Everything\Everything.exe')) or
    FileExists(ExpandConstant('{commonpf32}\Everything\Everything.exe')) or
    { A portable or scoop copy that is running answers the island just as well. }
    (FindWindowByClassName('EVERYTHING_TASKBAR_NOTIFICATION') <> 0);
end;

{ The component is ticked by default; an Everything already in place is never installed again,
  also when the components page was not shown (an update, a silent install). }
function WantEverything(): Boolean;
begin
  Result := WizardIsComponentSelected('everything') and not EverythingInstalled();
end;

function InitializeSetup(): Boolean;
var
  I: Integer;
begin
  FirstInstall := not FileExists(CoreExe());
  for I := 0 to ModuleCount - 1 do
    Installed[I] := FileExists(ExpandConstant('{localappdata}\Polynya\modules\') + ModuleId(I) + '\module.toml');
  Result := True;
end;

{ On an update the modules already there are ticked and the rest are not, also in a silent run. }
procedure InitializeWizard();
var
  I: Integer;
  Pick: String;
begin
  if FirstInstall then
    Exit;
  for I := 0 to ModuleCount - 1 do
  begin
    if not Installed[I] then
      Pick := Pick + '!';
    Pick := Pick + 'modules\' + ModuleId(I) + ',';
  end;
  WizardSelectComponents(Pick);
end;

{ An update goes straight to the point: no welcome, and no components page when there is nothing
  to add (every module and Everything are already there). }
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := ((PageID = wpWelcome) and not FirstInstall) or
    ((PageID = wpSelectComponents) and not FirstInstall and AllModulesInstalled() and EverythingInstalled());
end;

{ The "custom" type exists only to give the components their defaults; the summary leaves it out.
  The page appears on an update, where nothing else is shown before the Install button. }
function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo, MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
var
  I: Integer;
begin
  if FirstInstall then
  begin
    Result := MemoComponentsInfo + NewLine + NewLine + MemoTasksInfo;
    Exit;
  end;
  { An update lists only what it adds. }
  Result := FmtMessage(CustomMessage('UpdateTo'), ['{#AppVersion}']);
  for I := 0 to ModuleCount - 1 do
    if Added(I) then
      Result := Result + NewLine + Space + ModuleCaption(I);
  if WantEverything() then
    Result := Result + NewLine + Space + CustomMessage('Everything');
end;

{ Puts a grey note in the right column of the component named Caption. }
procedure Note(List: TNewCheckListBox; Caption, Text: String);
var
  I: Integer;
begin
  for I := 0 to List.Items.Count - 1 do
    if List.ItemCaption[I] = Caption then
      List.ItemSubItem[I] := Text;
end;

{ Greys out the component named Caption with an "already installed" note, ticked or not. }
procedure Lock(List: TNewCheckListBox; Caption: String; Ticked: Boolean);
var
  I: Integer;
begin
  for I := 0 to List.Items.Count - 1 do
    if List.ItemCaption[I] = Caption then
    begin
      List.Checked[I] := Ticked;
      List.ItemEnabled[I] := False;
      List.ItemSubItem[I] := CustomMessage('AlreadyInstalled');
    end;
end;

{ What is already there stays in the list, greyed out with a note, rather than vanishing: an
  installed module stays ticked (its files are updated), an installed Everything is left alone. }
procedure CurPageChanged(CurPageID: Integer);
var
  List: TNewCheckListBox;
  I: Integer;
begin
  if CurPageID <> wpSelectComponents then
    Exit;
  List := WizardForm.ComponentsList;
  Note(List, CustomMessage('ModAgents'), CustomMessage('NoteAgents'));
  Note(List, CustomMessage('ModNotify'), CustomMessage('NoteNotify'));
  for I := 0 to ModuleCount - 1 do
    if Installed[I] then
      Lock(List, ModuleCaption(I), True);
  if EverythingInstalled() then
    Lock(List, CustomMessage('Everything'), False)
  else
    Note(List, CustomMessage('Everything'), CustomMessage('NoteAdmin'));
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  { Waits until the island has exited and released its files. }
  if FileExists(CoreExe()) then
    Exec(CoreExe(), 'quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;

function Polynya(Params: String; Wait: TExecWait): Integer;
begin
  if not Exec(ExpandConstant('{app}\polynya.exe'), Params, '', SW_HIDE, Wait, Result) then
    Result := -1;
end;

{ Turns on the modules added in this run: an experimental one is off until enabled, and one the
  user turned off before (a reinstall that kept the settings) should not stay off once picked. }
procedure EnableAdded();
var
  Ids: String;
  I: Integer;
begin
  for I := 0 to ModuleCount - 1 do
    if Added(I) then
      Ids := Ids + ' ' + ModuleId(I);
  if Ids <> '' then
    Polynya('modules enable' + Ids, ewWaitUntilTerminated);
end;

{ Everything's own installer, silent. The service indexes NTFS without admin rights afterwards;
  the startup entry runs the Everything process that the island queries. }
procedure InstallEverything();
var
  Lang, Options: String;
  Code: Integer;
begin
  case ActiveLanguage() of
    'ru': Lang := '1049';
    'zh': Lang := '2052';
    'ja': Lang := '1041';
  else
    Lang := '1033';
  end;
  Options := '-app-data -disable-run-as-admin -install-service -install-run-on-system-startup -install-start-menu-shortcuts -language ' + Lang;
  if not ShellExec('runas', ExpandConstant('{tmp}\everything-setup.exe'), '/S -install-options "' + Options + '"', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then
  begin
    SuppressibleMsgBox(FmtMessage(CustomMessage('EverythingFailed'), [IntToStr(Code)]), mbError, MB_OK, IDOK);
    Exit;
  end;
  { The startup entry takes effect at the next sign-in; start it now so search works right away. }
  if (FindWindowByClassName('EVERYTHING_TASKBAR_NOTIFICATION') = 0) and FileExists(ExpandConstant('{commonpf64}\Everything\Everything.exe')) then
    Exec(ExpandConstant('{commonpf64}\Everything\Everything.exe'), '-startup', '', SW_HIDE, ewNoWait, Code);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep <> ssPostInstall then
    Exit;
  if WantEverything() then
    InstallEverything();
  EnableAdded();
  { The island speaks the language picked for the installer; the language names match ours. }
  if FirstInstall then
    Polynya('language ' + ActiveLanguage(), ewWaitUntilTerminated);
  { The package asks for UAC by itself; the island starts the module once it is in place. }
  if Added(2) then
    Polynya('notify-setup', ewWaitUntilTerminated);
  Polynya('', ewNoWait);
end;

function InitializeUninstall(): Boolean;
begin
  Purge := SuppressibleMsgBox(CustomMessage('Purge'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES;
  Result := True;
end;

{ Before the files go: the core closes the island and undoes autostart, agent hooks and the
  notifications package. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
  Params: String;
begin
  if CurUninstallStep <> usUninstall then
    Exit;
  Params := 'uninstall';
  if Purge then
    Params := Params + ' --purge';
  Exec(ExpandConstant('{app}\polynya.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, Code);
end;
