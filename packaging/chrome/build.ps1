# Build the NadsPDF Chrome extension:  dist\chrome-extension\ (load unpacked) and
# dist\release\nadspdf-<version>-chrome-extension.zip
#
#   powershell -File packaging\chrome\build.ps1 [-SkipBuild]
#
# Needs trunk (cargo install trunk --locked) and the wasm32-unknown-unknown target.
#
# Like Acrobat's extension, PDFs opened in Chrome show in NadsPDF instead of Chrome's viewer: a
# declarativeNetRequest rule (rules.json, no script) redirects a tab navigating to a .pdf URL to
# the extension's copy of NadsPDF for the web (apps/pdfcraft-web, WebAssembly), which downloads
# the file itself (`?file=`; the extension's host access lifts CORS). Everything runs in the
# browser: nothing is uploaded. There is no hand-written JavaScript: trunk generates the loader,
# which this script only moves out of index.html because extension pages forbid inline scripts.
#
# The manifest's public key (extension-key.txt) fixes the extension id, which the redirect rule
# has to name. A Chrome Web Store upload gets the store's own id: rebuild with -ExtensionId then.
param(
  [switch] $SkipBuild,
  [string] $ExtensionId = 'dkoohdpnfiphcimpklkphfbjibbaenje'
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Version = (Select-String -Path (Join-Path $Root 'Cargo.toml') -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$Out = Join-Path $Root 'dist\chrome-extension'
$Site = Join-Path $Root 'target\chrome-web'

if (-not $SkipBuild) {
  # trunk reads NO_COLOR as a boolean and rejects the common NO_COLOR=1.
  if ($env:NO_COLOR) { $env:NO_COLOR = 'true' }
  Push-Location (Join-Path $Root 'apps\pdfcraft-web')
  try {
    trunk build --release --dist $Site
    if ($LASTEXITCODE -ne 0) { throw "trunk build failed ($LASTEXITCODE)" }
  } finally { Pop-Location }
}
if (-not (Test-Path (Join-Path $Site 'index.html'))) { throw "$Site\index.html missing; run without -SkipBuild" }

Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Out | Out-Null
Copy-Item -Recurse (Join-Path $Site '*') $Out

# Extension pages run no inline script (Manifest V3): trunk's module loader goes to its own file.
$indexPath = Join-Path $Out 'index.html'
$index = [IO.File]::ReadAllText($indexPath)
$pattern = '(?s)<script type="module"[^>]*>(.*?)</script>'
$found = [regex]::Matches($index, $pattern)
if ($found.Count -ne 1) { throw "expected one inline module script in index.html, found $($found.Count)" }
[IO.File]::WriteAllText((Join-Path $Out 'loader.js'), $found[0].Groups[1].Value.Trim() + "`n")
$index = [regex]::Replace($index, $pattern, '<script type="module" src="./loader.js"></script>')
# Integrity hashes would no longer match the moved script; the files ship together anyway.
$index = [regex]::Replace($index, ' integrity="[^"]*"', '')
[IO.File]::WriteAllText($indexPath, $index)

# Icons: the app icon's PNG renders.
$Icons = Join-Path $Out 'icons'
New-Item -ItemType Directory -Force -Path $Icons | Out-Null
foreach ($s in 16, 32, 48, 128) {
  Copy-Item (Join-Path $Root "assets\app-icon\hicolor\${s}x${s}\apps\ai.storyteller.pdfcraft.png") (Join-Path $Icons "$s.png")
}
Copy-Item (Join-Path $PSScriptRoot 'popup.html') $Out

$key = (Get-Content (Join-Path $PSScriptRoot 'extension-key.txt') -Raw).Trim()
$manifest = [ordered]@{
  manifest_version = 3
  name = 'NadsPDF'
  short_name = 'NadsPDF'
  description = "Ouvre les PDF dans Chrome avec l'éditeur NadsPDF : lire, annoter, remplir, signer et modifier, sans rien envoyer sur Internet."
  version = ($Version -replace '-.*$', '')
  key = $key
  icons = [ordered]@{ '16' = 'icons/16.png'; '32' = 'icons/32.png'; '48' = 'icons/48.png'; '128' = 'icons/128.png' }
  action = [ordered]@{ default_title = 'NadsPDF'; default_popup = 'popup.html'; default_icon = [ordered]@{ '16' = 'icons/16.png'; '32' = 'icons/32.png' } }
  permissions = @('declarativeNetRequestWithHostAccess')
  host_permissions = @('<all_urls>')
  declarative_net_request = [ordered]@{ rule_resources = @([ordered]@{ id = 'pdf'; enabled = $true; path = 'rules.json' }) }
  web_accessible_resources = @([ordered]@{ resources = @('index.html', '*.js', '*.wasm', 'icons/*'); matches = @('<all_urls>') })
  content_security_policy = [ordered]@{ extension_pages = "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'" }
}
$utf8 = New-Object Text.UTF8Encoding $false
[IO.File]::WriteAllText((Join-Path $Out 'manifest.json'), ($manifest | ConvertTo-Json -Depth 8), $utf8)

# A tab opening a .pdf (web or file://) goes to NadsPDF with the address in ?file=. Only top-level
# navigations: PDFs embedded in pages, and NadsPDF's own download of the file, are left alone.
$rules = @(
  [ordered]@{
    id = 1
    priority = 1
    action = [ordered]@{ type = 'redirect'; redirect = [ordered]@{ regexSubstitution = "chrome-extension://$ExtensionId/index.html?file=\0" } }
    condition = [ordered]@{ regexFilter = '^(https?|file)://[^?#]*\.[pP][dD][fF]([?#].*)?$'; resourceTypes = @('main_frame') }
  }
)
[IO.File]::WriteAllText((Join-Path $Out 'rules.json'), (ConvertTo-Json -InputObject $rules -Depth 8), $utf8)

$Dist = Join-Path $Root 'dist\release'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Zip = Join-Path $Dist "nadspdf-$Version-chrome-extension.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $Out '*') -DestinationPath $Zip
Write-Host "wrote $Out (load unpacked) and $Zip"
