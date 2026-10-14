<#
.SYNOPSIS
  Publie une nouvelle version de NadsPDF.

.DESCRIPTION
  En une commande :
    1. met le numéro de version dans Cargo.toml (et Cargo.lock) ;
    2. construit l'installateur .msi et la version portable (packaging\windows\package.ps1),
       l'extension Chrome (packaging\chrome\build.ps1) et, si l'identité Store est remplie,
       le paquet .msix du Microsoft Store (packaging\windows\msix\build-msix.ps1) ;
    3. enregistre la version dans git (commit « Release vX.Y.Z » et étiquette vX.Y.Z) ;
    4. envoie le tout sur GitHub et crée la release nadsous/nadspdf avec le .msi, le .zip portable
       et l'extension ;
    5. vérifie que GitHub publie pour le .msi la même empreinte SHA-256 que le fichier construit :
       c'est elle que NadsPDF contrôle avant d'installer une mise à jour.

  Les copies de NadsPDF installées avec le .msi voient la nouvelle version au démarrage suivant
  (une vérification par jour) et proposent « Mettre à jour » : le .msi remplace l'ancienne version
  et garde les réglages. Le .msix n'est pas mis sur GitHub : il se téléverse dans Partner Center
  (voir packaging\windows\msix\PUBLIER-STORE.md).

  Le numéro doit être plus grand que celui de la version publiée, sinon Windows refuse de
  remplacer l'ancienne version.

  Il faut : Rust, WiX v5 (dotnet tool install --global wix --version 5.0.2), trunk et la cible
  wasm32 (extension), le Windows SDK (msix), et gh connecté au compte nadsous (gh auth login).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\release.ps1 0.7.2
  powershell -ExecutionPolicy Bypass -File packaging\release.ps1 0.7.2 -Notes "Correction de l'export Word"
  powershell -ExecutionPolicy Bypass -File packaging\release.ps1 0.7.2 -NoPublish   # construit seulement
#>
param(
  [Parameter(Mandatory = $true, Position = 0)] [string] $Version,
  # Le texte de la release (sinon la liste des commits depuis la version précédente).
  [string] $Notes,
  # Construire et enregistrer dans git, sans rien envoyer sur GitHub.
  [switch] $NoPublish,
  # Réutiliser les binaires déjà construits (pour reprendre après une erreur d'envoi).
  [switch] $SkipBuild,
  # Publier même si des fichiers modifiés ne sont pas enregistrés dans git.
  [switch] $AllowDirty,
  [string] $Repo = 'nadsous/nadspdf'
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Tag = "v$Version"
$Dist = Join-Path $Root 'dist\release'

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Host "==> $What" -ForegroundColor Cyan
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What a échoué (code $LASTEXITCODE)" }
}

function Get-CargoVersion {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
  }
  throw 'version introuvable dans [workspace.package] de Cargo.toml'
}

function ConvertTo-VersionKey([string] $v) {
  $core = ($v -split '[-+]')[0]
  if ($core -notmatch '^(\d+)\.(\d+)\.(\d+)$') { throw "« $v » n'est pas une version X.Y.Z" }
  return [version]"$($Matches[1]).$($Matches[2]).$($Matches[3])"
}

# ---- vérifications -----------------------------------------------------------------------------
$key = ConvertTo-VersionKey $Version
$current = Get-CargoVersion
Push-Location $Root
try {
  if (git tag --list $Tag) { throw "l'étiquette $Tag existe déjà : choisissez un numéro plus grand" }
  if (-not $NoPublish) {
    Invoke-Native 'gh auth status' { gh auth status }
    $latest = gh release view --repo $Repo --json tagName --jq .tagName 2>$null
    if ($LASTEXITCODE -eq 0 -and $latest -and ((ConvertTo-VersionKey $latest.TrimStart('v')) -ge $key)) {
      throw "la dernière release publiée est $latest : $Tag doit être plus grand pour que Windows remplace l'ancienne version"
    }
    $global:LASTEXITCODE = 0
  }
  # Ce qui est publié doit être ce qui est enregistré dans git.
  $dirty = git status --porcelain --untracked-files=no | Where-Object { $_ -notmatch ' (Cargo\.toml|Cargo\.lock)$' }
  if ($dirty -and -not $AllowDirty) {
    throw "des fichiers modifiés ne sont pas enregistrés dans git (git status). Faites un commit d'abord, ou relancez avec -AllowDirty."
  }
  $branch = (git rev-parse --abbrev-ref HEAD).Trim()
  Write-Host "NadsPDF $current -> $Version (branche $branch)" -ForegroundColor Green

  # ---- 1. numéro de version ----------------------------------------------------------------------
  if ($current -ne $Version) {
    $toml = Join-Path $Root 'Cargo.toml'
    $text = [IO.File]::ReadAllText($toml)
    $section = [regex]::Match($text, '(?ms)^\[workspace\.package\]\s*$.*?(?=^\[)')
    if (-not $section.Success) { throw '[workspace.package] introuvable dans Cargo.toml' }
    $updated = [regex]::Replace($section.Value, '(?m)^(\s*version\s*=\s*")[^"]+(")', "`${1}$Version`${2}", 1)
    $text = $text.Substring(0, $section.Index) + $updated + $text.Substring($section.Index + $section.Length)
    [IO.File]::WriteAllText($toml, $text, (New-Object Text.UTF8Encoding $false))
    # Cargo.lock suit (les crates du workspace seulement, sans réseau).
    Invoke-Native 'cargo metadata (Cargo.lock)' { cargo metadata --format-version 1 --offline | Out-Null }
  }

  # ---- 2. construction ---------------------------------------------------------------------------
  $ps = (Get-Process -Id $PID).Path
  $build = @()
  if ($SkipBuild) { $build = @('-SkipBuild') }
  Invoke-Native 'installateur .msi et version portable' { & $ps -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root 'packaging\windows\package.ps1') -Arch x64 @build }
  Invoke-Native 'extension Chrome' { & $ps -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root 'packaging\chrome\build.ps1') @build }
  $identity = Join-Path $Root 'packaging\windows\msix\store-identity.psd1'
  $msix = $null
  if ((Test-Path $identity) -and -not ((Get-Content $identity -Raw) -match 'REMPLACER-par')) {
    Invoke-Native 'paquet Microsoft Store .msix' { & $ps -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root 'packaging\windows\msix\build-msix.ps1') }
    $msix = Join-Path $Dist "nadspdf-$Version-windows-x64.msix"
  } else {
    Write-Warning "identité Store non remplie (packaging\windows\msix\store-identity.psd1) : pas de .msix cette fois"
  }

  $assets = @(
    (Join-Path $Dist "nadspdf-$Version-windows-x64.msi"),
    (Join-Path $Dist "nadspdf-$Version-windows-x64-portable.zip"),
    (Join-Path $Dist "nadspdf-$Version-chrome-extension.zip")
  )
  foreach ($a in $assets) { if (-not (Test-Path $a)) { throw "fichier attendu absent : $a" } }
  $msiHash = (Get-FileHash -Algorithm SHA256 $assets[0]).Hash.ToLowerInvariant()

  # ---- 3. git ------------------------------------------------------------------------------------
  Invoke-Native 'git add' { git add -- Cargo.toml Cargo.lock }
  git diff --cached --quiet
  if ($LASTEXITCODE -ne 0) {
    Invoke-Native 'git commit' { git commit -m "Release $Tag" }
  }
  $global:LASTEXITCODE = 0
  Invoke-Native "git tag $Tag" { git tag -a $Tag -m "NadsPDF $Version" }

  if ($NoPublish) {
    Write-Host ""
    Write-Host "Construit sans publier. Fichiers dans $Dist :" -ForegroundColor Green
    $assets + @($msix) | Where-Object { $_ } | ForEach-Object { Write-Host "  $_" }
    Write-Host "Pour publier ensuite : git push origin $branch $Tag, puis relancez avec -SkipBuild."
    return
  }

  # ---- 4. GitHub ---------------------------------------------------------------------------------
  Invoke-Native 'git push' { git push origin $branch }
  Invoke-Native "git push $Tag" { git push origin $Tag }
  $notesArgs = if ($Notes) { @('--notes', $Notes) } else { @('--generate-notes') }
  Invoke-Native "gh release create $Tag" {
    gh release create $Tag @assets --repo $Repo --title "NadsPDF $Version" --verify-tag @notesArgs
  }

  # ---- 5. empreinte publiée ----------------------------------------------------------------------
  $msiName = Split-Path $assets[0] -Leaf
  $release = gh api "repos/$Repo/releases/tags/$Tag" | Out-String | ConvertFrom-Json
  $published = ($release.assets | Where-Object { $_.name -eq $msiName } | Select-Object -First 1).digest
  if ($published -ne "sha256:$msiHash") {
    Write-Warning "GitHub publie « $published » pour $msiName, attendu sha256:$msiHash. NadsPDF refusera d'installer cette mise à jour : supprimez la release et recommencez."
  } else {
    Write-Host "ok empreinte du .msi : $msiHash" -ForegroundColor Green
  }

  Write-Host ""
  Write-Host "NadsPDF $Version est publié : https://github.com/$Repo/releases/tag/$Tag" -ForegroundColor Green
  if ($msix) { Write-Host "Reste à téléverser $msix dans Partner Center (voir PUBLIER-STORE.md)." }
} finally {
  Pop-Location
}
