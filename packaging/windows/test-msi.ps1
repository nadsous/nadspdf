<#
.SYNOPSIS
  Check the compiled MSI's install scope, publisher, shortcuts, native UI and OCR models, without installing it.
.EXAMPLE
  pwsh packaging/windows/test-msi.ps1 dist/release/pdfcraft-0.2.1-windows-x64.msi
#>
param([Parameter(Mandatory)] [string] $Path)
$ErrorActionPreference = 'Stop'
$Installer = New-Object -ComObject WindowsInstaller.Installer
$Database = $Installer.OpenDatabase((Resolve-Path -LiteralPath $Path).Path, 0)

function Read-Row([string] $Sql, [int] $Columns) {
  $view = $Database.OpenView($Sql)
  try {
    [void] $view.Execute()
    $record = $view.Fetch()
    if (-not $record) { throw "MSI row missing: $Sql" }
    $values = @(for ($i = 1; $i -le $Columns; $i++) { $record.StringData($i) })
    return ,$values
  } finally { [void] $view.Close() }
}

function Assert-Equal($Actual, $Expected, [string] $What) {
  if ($Actual -cne $Expected) { throw "$What`: expected '$Expected', got '$Actual'" }
}

function Assert-NoRow([string] $Sql, [string] $What) {
  $view = $Database.OpenView($Sql)
  try {
    [void] $view.Execute()
    if ($view.Fetch()) { throw "Unexpected MSI row ($What): $Sql" }
  } finally { [void] $view.Close() }
}

# Test the compiled condition with Windows Installer's evaluator, in a restricted session that
# cannot change machine state. Normal installs/repairs work, per-user overrides fail, and removal
# of an older incorrectly scoped installation remains possible (#305).
$scopeMessage = 'NadsPDF must be installed for all users. Run setup with administrator privileges and ALLUSERS=1; per-user installation is not supported.'
$scopeCondition = Read-Row ('SELECT `Condition` FROM `LaunchCondition` WHERE `Description` = ''' + $scopeMessage + '''') 1
$Installer.UILevel = 2
$session = $null
try {
  $session = $Installer.OpenPackage((Resolve-Path -LiteralPath $Path).Path, 1)
  foreach ($case in @(
      @('machine install', '1', '', '', '', 1),
      @('machine repair', '1', '', '1', '', 1),
      @('forced per-user install', '2', '1', '', '', 0),
      @('empty ALLUSERS', '', '', '', '', 0),
      @('resolved per-user install', '', '1', '', '', 0),
      @('forced per-user repair', '2', '1', '1', '', 0),
      @('machine uninstall', '1', '', '1', 'ALL', 1),
      @('legacy per-user uninstall', '', '1', '1', 'ALL', 1))) {
    $session.Property('ALLUSERS') = $case[1]
    $session.Property('MSIINSTALLPERUSER') = $case[2]
    $session.Property('Installed') = $case[3]
    $session.Property('REMOVE') = $case[4]
    Assert-Equal ($session.EvaluateCondition($scopeCondition[0])) $case[5] $case[0]
  }
} finally {
  if ($session) { [void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($session) }
}
foreach ($sequence in @('InstallUISequence', 'InstallExecuteSequence')) {
  $launch = Read-Row ('SELECT `Condition`, `Sequence` FROM `' + $sequence + '` WHERE `Action` = ''LaunchConditions''') 2
  Assert-Equal $launch[0] '' "$sequence launch conditions are unconditional"
  $cost = Read-Row ('SELECT `Sequence` FROM `' + $sequence + '` WHERE `Action` = ''CostInitialize''') 1
  if ([int] $launch[1] -le 0 -or [int] $launch[1] -ge [int] $cost[0]) {
    throw "$sequence must reject per-user overrides before costing"
  }
}
$manufacturer = Read-Row 'SELECT `Value` FROM `Property` WHERE `Property` = ''Manufacturer''' 1
Assert-Equal $manufacturer[0] 'nads' 'MSI manufacturer'
$status = Read-Row 'SELECT `Text` FROM `Control` WHERE `Dialog_` = ''InstallProgress'' AND `Control` = ''Status''' 1
Assert-Equal $status[0] 'Patientez pendant la mise en place de NadsPDF.' 'Persistent progress message'
Assert-NoRow 'SELECT `Event` FROM `EventMapping` WHERE `Dialog_` = ''InstallProgress'' AND `Control_` = ''Status''' 'progress text subscription'

# Plain (non-advertised) shortcuts to nadspdf.exe, each in its own component; the desktop one is
# gated by INSTALLDESKTOPSHORTCUT, which is secure so the UI choice reaches the
# elevated install.
foreach ($entry in @(@('StartMenuShortcut', 'ProgramMenuFolder', 'PdfcraftStartMenuShortcut', ''),
                     @('DesktopShortcut', 'DesktopFolder', 'PdfcraftDesktopShortcut', 'INSTALLDESKTOPSHORTCUT = 1'))) {
  $row = Read-Row ('SELECT `Directory_`, `Target`, `Icon_`, `WkDir`, `Component_` FROM `Shortcut` WHERE `Shortcut` = ''' + $entry[0] + '''') 5
  Assert-Equal $row[0] $entry[1] "$($entry[0]) directory"
  Assert-Equal $row[1] '[#PdfcraftExe]' "$($entry[0]) target"
  Assert-Equal $row[2] 'PdfcraftIcon.ico' "$($entry[0]) icon"
  Assert-Equal $row[3] 'INSTALLFOLDER' "$($entry[0]) working directory"
  Assert-Equal $row[4] $entry[2] "$($entry[0]) component"
  $component = Read-Row ('SELECT `Directory_`, `Condition` FROM `Component` WHERE `Component` = ''' + $entry[2] + '''') 2
  Assert-Equal $component[0] $entry[1] "$($entry[2]) directory"
  Assert-Equal $component[1] $entry[3] "$($entry[2]) condition"
}
# No static default (#1058): a SetProperty after AppSearch turns it on for a first install, and for an
# update only while the old shortcut is still on the desktop; a command-line value is left alone.
Assert-NoRow 'SELECT `Value` FROM `Property` WHERE `Property` = ''INSTALLDESKTOPSHORTCUT''' 'desktop shortcut static default'
$setDesktop = Read-Row 'SELECT `Action`, `Target` FROM `CustomAction` WHERE `Source` = ''INSTALLDESKTOPSHORTCUT''' 2
Assert-Equal $setDesktop[1] '1' 'Desktop shortcut default value'
foreach ($sequence in @('InstallUISequence', 'InstallExecuteSequence')) {
  $search = Read-Row ('SELECT `Sequence` FROM `' + $sequence + '` WHERE `Action` = ''AppSearch''') 1
  $set = Read-Row ('SELECT `Sequence`, `Condition` FROM `' + $sequence + '` WHERE `Action` = ''' + $setDesktop[0] + '''') 2
  if ([int] $set[0] -le [int] $search[0]) { throw "$sequence must set the desktop shortcut default after AppSearch" }
}
$session = $null
try {
  $session = $Installer.OpenPackage((Resolve-Path -LiteralPath $Path).Path, 1)
  foreach ($case in @(
      @('first install', '', '', '', 1),
      @('first install, INSTALLDESKTOPSHORTCUT=0', '0', '', '', 0),
      @('update, shortcut still there', '', '{00000000-0000-0000-0000-000000000000}', 'C:\Users\Public\Desktop\NadsPDF.lnk', 1),
      @('update, shortcut deleted', '', '{00000000-0000-0000-0000-000000000000}', '', 0),
      @('update, INSTALLDESKTOPSHORTCUT=0', '0', '{00000000-0000-0000-0000-000000000000}', 'C:\Users\Public\Desktop\NadsPDF.lnk', 0))) {
    $session.Property('INSTALLDESKTOPSHORTCUT') = $case[1]
    $session.Property('WIX_UPGRADE_DETECTED') = $case[2]
    $session.Property('EXISTINGDESKTOPSHORTCUT') = $case[3]
    Assert-Equal ($session.EvaluateCondition($set[1])) $case[4] "Desktop shortcut default: $($case[0])"
  }
} finally {
  if ($session) { [void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($session) }
}
$secure = Read-Row 'SELECT `Value` FROM `Property` WHERE `Property` = ''SecureCustomProperties''' 1
if (($secure[0] -split ';') -notcontains 'INSTALLDESKTOPSHORTCUT') { throw "INSTALLDESKTOPSHORTCUT is not secure: '$($secure[0])'" }
$checkbox = Read-Row 'SELECT `Type`, `Property`, `Text` FROM `Control` WHERE `Dialog_` = ''InstallWelcome'' AND `Control` = ''DesktopShortcut''' 3
Assert-Equal $checkbox[0] 'CheckBox' 'Welcome desktop-shortcut control'
Assert-Equal $checkbox[1] 'INSTALLDESKTOPSHORTCUT' 'Welcome checkbox property'
if ($checkbox[2] -notmatch 'raccourci sur le Bureau') { throw "Welcome checkbox label: '$($checkbox[2])'" }
$checked = Read-Row 'SELECT `Value` FROM `CheckBox` WHERE `Property` = ''INSTALLDESKTOPSHORTCUT''' 1
Assert-Equal $checked[0] '1' 'Welcome checkbox value'
$app = Read-Row 'SELECT `KeyPath` FROM `Component` WHERE `Component` = ''PdfcraftApp''' 1
Assert-Equal $app[0] 'PdfcraftExe' 'Shortcut executable key path'
$scope = Read-Row 'SELECT `Value` FROM `Property` WHERE `Property` = ''ALLUSERS''' 1
Assert-Equal $scope[0] '1' 'Per-machine shortcut scope'

# File Explorer right-click menu (#682, #190): convert, combine and open entries in their own
# component, gated by INSTALLPDFCONTEXTMENU (default 1, secure, welcome checkbox), with a menu value
# as key path so uninstall removes them. The same keys and values as the app's per-user
# registration (crates/platform/src/explorer_menu.rs). Several selected files each start
# nadspdf.exe, which the single-instance hand-off gathers into one window (one Combine files list
# for --combine). No file type's default app is changed.
$convertible = @('png', 'jpg', 'jpeg', 'tif', 'tiff', 'gif', 'bmp', 'jp2', 'j2k', 'jpx', 'txt', 'text', 'docx', 'xlsx', 'xlsm', 'ods', 'csv', 'tsv', 'html', 'htm')
$verbs = @(
  @('NadsPDF.Convert', $convertible, 'Convertir en PDF avec NadsPDF', 'Document', '"[#PdfcraftExe]" "%1"'),
  @('NadsPDF.Combine', (@('pdf') + $convertible), 'Fusionner avec NadsPDF', 'Player', '"[#PdfcraftExe]" --combine "%1"'),
  @('NadsPDF.Open', @('pdf'), 'Ouvrir avec NadsPDF', 'Document', '"[#PdfcraftExe]" "%1"'))
foreach ($verb in $verbs) {
  foreach ($ext in $verb[1]) {
    $key = 'Software\Classes\SystemFileAssociations\.' + $ext + '\shell\' + $verb[0]
    foreach ($value in @(@('MUIVerb', $verb[2]), @('Icon', '"[#PdfcraftExe]",0'), @('MultiSelectModel', $verb[3]))) {
      $row = Read-Row ('SELECT `Value`, `Component_`, `Root` FROM `Registry` WHERE `Key` = ''' + $key + ''' AND `Name` = ''' + $value[0] + '''') 3
      Assert-Equal $row[0] $value[1] "$ext $($verb[0]) $($value[0])"
      Assert-Equal $row[1] 'NadspdfExplorerMenu' "$ext $($verb[0]) $($value[0]) component"
      Assert-Equal $row[2] '2' "$ext $($verb[0]) HKLM root"
    }
    $command = Read-Row ('SELECT `Value`, `Component_` FROM `Registry` WHERE `Key` = ''' + $key + '\command''') 2
    Assert-Equal $command[0] $verb[4] "$ext $($verb[0]) command"
    Assert-Equal $command[1] 'NadspdfExplorerMenu' "$ext $($verb[0]) command component"
  }
}
# The entries they replace are gone.
Assert-NoRow 'SELECT `Registry` FROM `Registry` WHERE `Key` = ''Software\Classes\SystemFileAssociations\.png\shell\PdfCraft.CreatePdf''' 'old image menu'
Assert-NoRow 'SELECT `Registry` FROM `Registry` WHERE `Key` = ''Software\Classes\SystemFileAssociations\.pdf\shell\PdfCraft.Edit''' 'old PDF menu'
$menuKeyPath = Read-Row 'SELECT `Registry` FROM `Registry` WHERE `Key` = ''Software\Classes\SystemFileAssociations\.pdf\shell\NadsPDF.Open'' AND `Name` = ''MUIVerb''' 1
$menuComponent = Read-Row 'SELECT `Condition`, `KeyPath` FROM `Component` WHERE `Component` = ''NadspdfExplorerMenu''' 2
Assert-Equal $menuComponent[0] 'INSTALLPDFCONTEXTMENU = 1' 'Explorer menu condition'
Assert-Equal $menuComponent[1] $menuKeyPath[0] 'Explorer menu key path'
$pdfDefault = Read-Row 'SELECT `Value` FROM `Property` WHERE `Property` = ''INSTALLPDFCONTEXTMENU''' 1
Assert-Equal $pdfDefault[0] '1' 'Explorer menu default'
if (($secure[0] -split ';') -notcontains 'INSTALLPDFCONTEXTMENU') { throw "INSTALLPDFCONTEXTMENU is not secure: '$($secure[0])'" }
$pdfCheckbox = Read-Row 'SELECT `Type`, `Property`, `Text` FROM `Control` WHERE `Dialog_` = ''InstallWelcome'' AND `Control` = ''PdfContextMenu''' 3
Assert-Equal $pdfCheckbox[0] 'CheckBox' 'Welcome Explorer-menu control'
Assert-Equal $pdfCheckbox[1] 'INSTALLPDFCONTEXTMENU' 'Welcome Explorer-menu property'
if ($pdfCheckbox[2] -notmatch 'clic droit') { throw "Welcome Explorer-menu label: '$($pdfCheckbox[2])'" }
$pdfChecked = Read-Row 'SELECT `Value` FROM `CheckBox` WHERE `Property` = ''INSTALLPDFCONTEXTMENU''' 1
Assert-Equal $pdfChecked[0] '1' 'Welcome Explorer-menu value'
Assert-NoRow 'SELECT `Registry` FROM `Registry` WHERE `Key` = ''Software\Classes\.pdf'' AND `Name` IS NULL' '.pdf default app'

# Negative sequences are Windows Installer's success/user-exit/failure paths. Only full UI
# shows these dialogs: an unattended /qn or /qb install must never wait for a Finish click.
foreach ($exit in @(@('InstallComplete', '-1'), @('InstallCancelled', '-2'), @('InstallFailed', '-3'))) {
  $row = Read-Row ('SELECT `Condition`, `Sequence` FROM `InstallUISequence` WHERE `Action` = ''' + $exit[0] + '''') 2
  Assert-Equal $row[0] 'UILevel = 5' "$($exit[0]) UI level"
  Assert-Equal $row[1] $exit[1] "$($exit[0]) exit path"
  $finish = Read-Row ('SELECT `Type`, `Text` FROM `Control` WHERE `Dialog_` = ''' + $exit[0] + ''' AND `Control` = ''Finish''') 2
  Assert-Equal $finish[0] 'PushButton' "$($exit[0]) Finish control"
  Assert-Equal $finish[1] '&Terminer' "$($exit[0]) Finish label"
  $endDialog = Read-Row ('SELECT `Argument` FROM `ControlEvent` WHERE `Dialog_` = ''' + $exit[0] + ''' AND `Control_` = ''Finish'' AND `Event` = ''EndDialog''') 1
  Assert-Equal $endDialog[0] 'Return' "$($exit[0]) Finish event"
}
$title = Read-Row 'SELECT `Text` FROM `Control` WHERE `Dialog_` = ''InstallComplete'' AND `Control` = ''Title''' 1
if ($title[0] -notmatch 'NadsPDF est pr') { throw 'Success dialog does not confirm completion' }
# OCR models (#103): installed into models\ beside nadspdf.exe, where the app looks for them.
$modelsDir = Read-Row 'SELECT `Directory_Parent`, `DefaultDir` FROM `Directory` WHERE `Directory` = ''ModelsFolder''' 2
Assert-Equal $modelsDir[0] 'INSTALLFOLDER' 'OCR models folder parent'
# DefaultDir is `models`, or `SHORT|models` with a generated 8.3 name.
if ($modelsDir[1] -notmatch '(^|\|)models$') { throw "OCR models folder name: '$($modelsDir[1])'" }
$modelFiles = 0
$view = $Database.OpenView('SELECT `File`.`FileName` FROM `File`, `Component` WHERE `File`.`Component_` = `Component`.`Component` AND `Component`.`Directory_` = ''ModelsFolder''')
try {
  [void] $view.Execute()
  while ($record = $view.Fetch()) { if ($record.StringData(1) -match '\.rten$') { $modelFiles++ } }
} finally { [void] $view.Close() }
if ($modelFiles -lt 2) { throw "expected the OCR models (*.rten) in models\, found $modelFiles" }
$rm = Read-Row 'SELECT `Dialog` FROM `Dialog` WHERE `Dialog` = ''MsiRMFilesInUse''' 1
Assert-Equal $rm[0] 'MsiRMFilesInUse' 'Files-in-use dialog'
[void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($Database)
[void] [Runtime.InteropServices.Marshal]::FinalReleaseComObject($Installer)
Write-Output 'ok MSI: per-machine scope guard, publisher, persistent progress text, Start Menu shortcut, optional desktop shortcut (on for a first install, kept on update only while present, checkbox), optional PDF context menu (default on, checkbox), icon/key path, full-UI success/cancel/error and Finish controls, files-in-use dialog, OCR models'
