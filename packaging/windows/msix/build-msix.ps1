<#
.SYNOPSIS
  Package NadsPDF as an MSIX for the Microsoft Store.

.DESCRIPTION
  Takes the files package.ps1 staged in target\windows-package\<arch>\ (nadspdf.exe,
  pdfcraft-cli.exe, licences, models\), generates the MSIX visual assets from
  assets\app-icon\, fills AppxManifest.xml, builds resources.pri (makepri) and packs
  dist\release\nadspdf-<version>-windows-<arch>.msix (makeappx, which also validates the
  manifest against the schema).

  The package is left unsigned: the Microsoft Store signs what it publishes. For a local
  sideload test, pass -PfxPath (a certificate whose subject equals -Publisher).

  The Store identity (Partner Center > the app > Product management > Product identity) comes
  from -IdentityName / -Publisher / -PublisherDisplayName, or else from store-identity.psd1
  beside this script. -Test uses a placeholder identity (CN=NadsPDF Test) and writes
  ...-test.msix, which the Store would reject.

  Needs the Windows SDK (makeappx.exe, makepri.exe; signtool.exe only with -PfxPath).
  Does not build anything: run packaging\windows\package.ps1 first.

.EXAMPLE
  powershell -File packaging\windows\msix\build-msix.ps1 -Test
  powershell -File packaging\windows\msix\build-msix.ps1
  powershell -File packaging\windows\msix\build-msix.ps1 -IdentityName 12345Nads.NadsPDF -Publisher "CN=1A2B..." -PublisherDisplayName nads
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [string] $IdentityName,
  [string] $Publisher,
  [string] $PublisherDisplayName,
  [switch] $Test,
  [string] $PfxPath,
  [string] $PfxPassword
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# ---- version -----------------------------------------------------------------------------------
# One source: [workspace.package] version in Cargo.toml. MSIX wants a.b.c.d with d = 0 for the
# Store (the Store reserves the last field); pre-release tags are dropped.
$Version = $env:PDFCRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
$core = ($Version -split '[-+]')[0]
if ($core -notmatch '^(\d+)\.(\d+)\.(\d+)$') { throw "version '$Version' is not major.minor.patch" }
foreach ($n in $Matches[1], $Matches[2], $Matches[3]) {
  if ([int64]$n -gt 65535) { throw "version '$Version': each MSIX version field must be <= 65535" }
}
$MsixVersion = "$core.0"

# ---- identity ----------------------------------------------------------------------------------
if ($Test) {
  if (-not $IdentityName) { $IdentityName = 'NadsPDF.Test' }
  if (-not $Publisher) { $Publisher = 'CN=NadsPDF Test' }
  if (-not $PublisherDisplayName) { $PublisherDisplayName = 'NadsPDF Test' }
} else {
  $cfgPath = Join-Path $PSScriptRoot 'store-identity.psd1'
  if (Test-Path $cfgPath) {
    $cfg = Import-PowerShellDataFile $cfgPath
    if (-not $IdentityName -and $cfg.ContainsKey('IdentityName')) { $IdentityName = $cfg.IdentityName }
    if (-not $Publisher -and $cfg.ContainsKey('Publisher')) { $Publisher = $cfg.Publisher }
    if (-not $PublisherDisplayName -and $cfg.ContainsKey('PublisherDisplayName')) { $PublisherDisplayName = $cfg.PublisherDisplayName }
  }
  $missing = @()
  foreach ($p in @(@('IdentityName', $IdentityName), @('Publisher', $Publisher), @('PublisherDisplayName', $PublisherDisplayName))) {
    if (-not $p[1] -or $p[1] -match 'REMPLACER') { $missing += $p[0] }
  }
  if ($missing.Count -gt 0) {
    throw ("Store identity missing: $($missing -join ', '). Copy the values from Partner Center > " +
      "NadsPDF > Product management > Product identity into $cfgPath (or pass -IdentityName, " +
      "-Publisher, -PublisherDisplayName). Use -Test for a local test package.")
  }
}
# Package/Identity/Name: 3-50 characters, letters, digits, dots and hyphens.
if ($IdentityName -notmatch '^[A-Za-z0-9.\-]{3,50}$') { throw "IdentityName '$IdentityName' must be 3-50 characters of A-Z a-z 0-9 . -" }
if ($Publisher -notmatch '^CN=') { throw "Publisher '$Publisher' must be a distinguished name starting with CN=" }

$Suffix = if ($Test) { '-test' } else { '' }
Write-Output "NadsPDF $Version (MSIX $MsixVersion) for Windows $Arch"
Write-Output "Identity: $IdentityName / $Publisher / $PublisherDisplayName"

# ---- Windows SDK tools -------------------------------------------------------------------------
function Find-SdkTool([string] $Exe) {
  $hostDir = if ([Environment]::Is64BitOperatingSystem) { 'x64' } else { 'x86' }
  $bin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  $candidates = @()
  if (Test-Path $bin) {
    $versioned = Get-ChildItem $bin -Directory | Where-Object { $_.Name -match '^\d+(\.\d+){3}$' } |
      Sort-Object { [version]$_.Name } -Descending
    foreach ($d in $versioned) { $candidates += (Join-Path $d.FullName "$hostDir\$Exe") }
    $candidates += (Join-Path $bin "$hostDir\$Exe")
  }
  foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
  $onPath = Get-Command $Exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  throw "$Exe not found: install the Windows 10/11 SDK"
}
$MakeAppx = Find-SdkTool 'makeappx.exe'
$MakePri = Find-SdkTool 'makepri.exe'
Write-Output "makeappx: $MakeAppx"

# ---- inputs ------------------------------------------------------------------------------------
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
$Staged = Join-Path $TargetDir "windows-package\$Arch"
foreach ($f in 'nadspdf.exe', 'pdfcraft-cli.exe', 'models\ATTRIBUTION.txt') {
  if (-not (Test-Path (Join-Path $Staged $f))) {
    throw "$Staged\$f is missing: run packaging\windows\package.ps1 -Arch $Arch first"
  }
}
# The exe must be for the package's architecture (an x64 exe must never ship as arm64, #61).
$bytes = [System.IO.File]::ReadAllBytes((Join-Path $Staged 'nadspdf.exe'))
$pe = [BitConverter]::ToInt32($bytes, 0x3C)
$machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
$bytes = $null
$want = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
if ($machine -ne $want) { throw "nadspdf.exe is for machine 0x$('{0:X}' -f $machine), expected $Arch" }

$Work = Join-Path $TargetDir "msix\$Arch"
$Pkg = Join-Path $Work 'package'
$PriDir = Join-Path $Work 'pri'
Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Pkg, $PriDir | Out-Null
Copy-Item -Recurse (Join-Path $Staged '*') $Pkg

# ---- visual assets -----------------------------------------------------------------------------
# Generated from the committed icons on every build, so nothing new is committed. The hicolor PNGs
# are the full-bleed tile at exact sizes (pixel-tuned small sizes); the document icon's ICO frames
# are PNGs.
Add-Type -AssemblyName System.Drawing
$Assets = Join-Path $Pkg 'Assets'
New-Item -ItemType Directory -Force -Path $Assets | Out-Null
$IconDir = Join-Path $Root 'assets\app-icon'

function Get-HicolorPath([int] $Size) { Join-Path $IconDir "hicolor\${Size}x${Size}\apps\ai.storyteller.pdfcraft.png" }

# PNG frames of an .ico, by width (0 in the directory means 256).
function Get-IcoPngFrames([string] $Path) {
  $b = [System.IO.File]::ReadAllBytes($Path)
  $frames = @{}
  $count = [BitConverter]::ToUInt16($b, 4)
  for ($k = 0; $k -lt $count; $k++) {
    $o = 6 + 16 * $k
    if ($o + 16 -gt $b.Length) { break }
    $w = [int]$b[$o]; if ($w -eq 0) { $w = 256 }
    $size = [BitConverter]::ToUInt32($b, $o + 8)
    $off = [BitConverter]::ToUInt32($b, $o + 12)
    if ($size -lt 8 -or $off + $size -gt $b.Length -or $b[$off] -ne 0x89) { continue }
    $png = New-Object byte[] $size
    [Array]::Copy($b, [int64]$off, $png, 0, [int64]$size)
    $frames[$w] = $png
  }
  return $frames
}

function Open-Image([string] $Path) {
  # Load through a copy so the file isn't kept locked.
  $tmp = [System.Drawing.Image]::FromFile($Path)
  try { return New-Object System.Drawing.Bitmap $tmp } finally { $tmp.Dispose() }
}

# Draws $Src centred in a transparent $W x $H canvas, scaled so its side is $Fill of the shorter edge.
function Save-Scaled([System.Drawing.Image] $Src, [int] $W, [int] $H, [double] $Fill, [string] $Out) {
  $bmp = New-Object System.Drawing.Bitmap $W, $H, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $attr = New-Object System.Drawing.Imaging.ImageAttributes
  try {
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $attr.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY) # no dark fringe at the edges
    $side = [int][Math]::Round([Math]::Min($W, $H) * $Fill)
    $rect = New-Object System.Drawing.Rectangle ([int](($W - $side) / 2)), ([int](($H - $side) / 2)), $side, $side
    $g.DrawImage($Src, $rect, 0, 0, $Src.Width, $Src.Height, [System.Drawing.GraphicsUnit]::Pixel, $attr)
  } finally { $attr.Dispose(); $g.Dispose() }
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}

$AppMaster = Open-Image (Get-HicolorPath 512)
$DocFrames = Get-IcoPngFrames (Join-Path $IconDir 'pdfcraft-document.ico')
if (-not $DocFrames.ContainsKey(256)) { throw 'pdfcraft-document.ico has no 256 px PNG frame' }
$DocMasterPath = Join-Path $Work 'doc-256.png'
[System.IO.File]::WriteAllBytes($DocMasterPath, $DocFrames[256])
$DocMaster = Open-Image $DocMasterPath
try {
  # Tiles: the icon on the #111113 BackgroundColor, with some margin.
  $tiles = @(
    @('Square150x150Logo', 150, 150, 0.8), @('Square71x71Logo', 71, 71, 0.8),
    @('Square310x310Logo', 310, 310, 0.8), @('Wide310x150Logo', 310, 150, 0.8)
  )
  foreach ($t in $tiles) {
    foreach ($scale in 100, 200) {
      $w = [int]($t[1] * $scale / 100); $h = [int]($t[2] * $scale / 100)
      Save-Scaled $AppMaster $w $h $t[3] (Join-Path $Assets "$($t[0]).scale-$scale.png")
    }
  }
  # Full-bleed icons: Store logo, app list / taskbar icon, PDF file icon.
  foreach ($scale in 100, 200, 400) {
    Save-Scaled $AppMaster ([int](50 * $scale / 100)) ([int](50 * $scale / 100)) 1.0 (Join-Path $Assets "StoreLogo.scale-$scale.png")
    Save-Scaled $AppMaster ([int](44 * $scale / 100)) ([int](44 * $scale / 100)) 1.0 (Join-Path $Assets "Square44x44Logo.scale-$scale.png")
    Save-Scaled $DocMaster ([int](44 * $scale / 100)) ([int](44 * $scale / 100)) 1.0 (Join-Path $Assets "PdfDocument.scale-$scale.png")
  }
  # targetsize-N (plated, Windows 10 Start list) and _altform-unplated (taskbar, Windows 11 Start,
  # File Explorer); both are the full tile, which has its own background.
  foreach ($size in 16, 24, 32, 48, 256) {
    foreach ($q in "targetsize-$size", "targetsize-${size}_altform-unplated") {
      Copy-Item (Get-HicolorPath $size) (Join-Path $Assets "Square44x44Logo.$q.png")
      $docOut = Join-Path $Assets "PdfDocument.$q.png"
      if ($DocFrames.ContainsKey($size)) { [System.IO.File]::WriteAllBytes($docOut, $DocFrames[$size]) }
      else { Save-Scaled $DocMaster $size $size 1.0 $docOut }
    }
  }
} finally { $AppMaster.Dispose(); $DocMaster.Dispose() }
$big = Get-ChildItem $Assets | Where-Object { $_.Length -gt 200KB }
if ($big) { throw "MSIX images must be under 200 KB: $($big.Name -join ', ')" }

# ---- manifest ----------------------------------------------------------------------------------
$esc = { param($s) [System.Security.SecurityElement]::Escape($s) }
$manifest = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'AppxManifest.xml'), [System.Text.Encoding]::UTF8)
$tokens = [ordered]@{
  '{{IDENTITY_NAME}}' = (& $esc $IdentityName)
  '{{PUBLISHER}}' = (& $esc $Publisher)
  '{{PUBLISHER_DISPLAY_NAME}}' = (& $esc $PublisherDisplayName)
  '{{VERSION}}' = $MsixVersion
  '{{ARCH}}' = $Arch
}
foreach ($k in $tokens.Keys) { $manifest = $manifest.Replace($k, $tokens[$k]) }
if ($manifest -match '\{\{[A-Z_]+\}\}') { throw "unfilled manifest token $($Matches[0])" }
$null = [xml]$manifest # well-formed before makeappx's schema check
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText((Join-Path $Pkg 'AppxManifest.xml'), $manifest, $utf8)

# ---- resources.pri -----------------------------------------------------------------------------
# Resolves Assets\X.png to its scale-/targetsize- variants. Indexed from a copy holding only the
# manifest and the images, so the 100 MB of binaries and models don't become resource entries.
Copy-Item -Recurse $Assets (Join-Path $PriDir 'Assets')
Copy-Item (Join-Path $Pkg 'AppxManifest.xml') $PriDir
$PriConfig = Join-Path $Work 'priconfig.xml'
Invoke-Native 'makepri createconfig' { & $MakePri createconfig /cf $PriConfig /dq fr-FR /pv 10.0.0 /o | Out-Null }
# One resources.pri for this single package: drop the per-scale/language split (for bundles).
$cfgXml = [xml][System.IO.File]::ReadAllText($PriConfig)
foreach ($node in @($cfgXml.SelectNodes('//packaging'))) { $null = $node.ParentNode.RemoveChild($node) }
$cfgXml.Save($PriConfig)
Invoke-Native 'makepri new' { & $MakePri new /pr $PriDir /cf $PriConfig /mn (Join-Path $PriDir 'AppxManifest.xml') /of (Join-Path $Pkg 'resources.pri') /o | Out-Null }

# ---- pack --------------------------------------------------------------------------------------
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Msix = Join-Path $Dist "nadspdf-$Version-windows-$Arch$Suffix.msix"
Remove-Item -Force $Msix -ErrorAction SilentlyContinue
# makeappx validates the manifest against the schema and checks every file it names.
Invoke-Native 'makeappx pack' {
  & $MakeAppx pack /d $Pkg /p $Msix /o | Where-Object { $_ -and $_ -notmatch '^Processing ' }
}

if ($PfxPath) {
  $SignTool = Find-SdkTool 'signtool.exe'
  $signArgs = @('sign', '/fd', 'SHA256', '/f', $PfxPath)
  if ($PfxPassword) { $signArgs += @('/p', $PfxPassword) }
  Invoke-Native 'signtool sign (local test certificate)' { & $SignTool @signArgs $Msix }
} else {
  Write-Output 'Unsigned: the Microsoft Store signs the package on publication.'
}
Get-Item $Msix | Format-Table Name, Length
