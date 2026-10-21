# Build the NadsPDF Chrome extension:  dist\chrome-extension\ (load unpacked) and
# dist\release\nadspdf-<version>-chrome-extension.zip
#
#   powershell -File packaging\chrome\build.ps1 [-SkipBuild]
#   powershell -File packaging\chrome\build.ps1 -Store chrome -ExtensionId <id from the store>
#   powershell -File packaging\chrome\build.ps1 -Store chrome -FirstUpload   (only to get that id)
#
# Needs trunk (cargo install trunk --locked) and the wasm32-unknown-unknown target.
#
# Like Acrobat's extension, PDFs opened in Chrome show in NadsPDF instead of Chrome's viewer: a
# declarativeNetRequest rule set (rules.json, no script) redirect a tab navigating to a PDF (a .pdf
# URL, or a page served as application/pdf) to the extension's copy of NadsPDF for the web
# (apps/pdfcraft-web, WebAssembly), which downloads the file itself (`#file=`; the extension's
# host access lifts CORS, and it reads file:// through XHR). Everything runs in the
# browser: nothing is uploaded. There is no hand-written JavaScript: trunk generates the loader,
# which this script only moves out of index.html because extension pages forbid inline scripts.
#
# The manifest's public key (extension-key.txt) fixes the extension id, which the redirect rule
# has to name. Each store gives the extension its own id instead (and the Chrome Web Store refuses
# a manifest with a key): -Store chrome|edge leaves the key out and needs that id in -ExtensionId
# (packaging\chrome\PUBLISHING.md). A store package naming the wrong id installs fine and opens no
# PDF (the redirect lands on a missing extension: ERR_BLOCKED_BY_CLIENT), so -Store refuses to
# build without -ExtensionId unless -FirstUpload says the package only serves to obtain that id.
param(
  [switch] $SkipBuild,
  [string] $ExtensionId = 'dkoohdpnfiphcimpklkphfbjibbaenje',
  [ValidateSet('', 'chrome', 'edge')] [string] $Store = '',
  [switch] $FirstUpload,
  # A store package of the same app version again (the store refuses a version it already has):
  # the manifest says <version>.<Revision>, such as 0.7.6.1.
  [ValidateRange(0, 65535)] [int] $Revision = 0
)
$ErrorActionPreference = 'Stop'
if ($Store -and -not $PSBoundParameters.ContainsKey('ExtensionId')) {
  if (-not $FirstUpload) {
    throw "-Store $Store needs -ExtensionId <the id the store shows> (or -FirstUpload, only to obtain that id; PUBLISHING.md)"
  }
  Write-Warning "first upload only: PDFs won't open until you rebuild with -Store $Store -ExtensionId <the id the store shows> (PUBLISHING.md)"
}
if ($ExtensionId -cnotmatch '^[a-p]{32}$') { throw "-ExtensionId must be 32 letters a-p, got '$ExtensionId'" }
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

# Scan & OCR: the recognition models (about 12 MB, ATTRIBUTION.toml), which the page fetches from
# models/ only when the user first opens Recognize Text (apps/pdfcraft-web `ocr_models_fetcher`).
$ModelDir = if ($env:PDFCRAFT_MODELS) { $env:PDFCRAFT_MODELS } else { Join-Path $Root 'assets\models' }
$Models = Join-Path $Out 'models'
New-Item -ItemType Directory -Force -Path $Models | Out-Null
foreach ($m in 'text-detection.rten', 'text-recognition.rten') {
  $src = Join-Path $ModelDir $m
  if (-not (Test-Path $src)) { throw "$src missing: run cargo xtask models (or set PDFCRAFT_MODELS)" }
  Copy-Item $src $Models
}
# CC-BY-SA-4.0 asks for the credit and the licence to travel with the models, as in the MSI.
Get-ChildItem $ModelDir -Filter '*.txt' | Copy-Item -Destination $Models

$key = (Get-Content (Join-Path $PSScriptRoot 'extension-key.txt') -Raw).Trim()
$manifest = [ordered]@{
  manifest_version = 3
  name = 'NadsPDF'
  short_name = 'NadsPDF'
  description = "Ouvre les PDF dans Chrome avec NadsPDF : lire, annoter, remplir, signer, modifier et OCR, sans rien envoyer sur Internet."
  version = ($Version -replace '-.*$', '') + $(if ($Revision) { ".$Revision" } else { '' })
  # Rule 3 matches on response headers, which older browsers would ignore and so redirect every page.
  minimum_chrome_version = '128'
  icons = [ordered]@{ '16' = 'icons/16.png'; '32' = 'icons/32.png'; '48' = 'icons/48.png'; '128' = 'icons/128.png' }
  action = [ordered]@{ default_title = 'NadsPDF'; default_popup = 'popup.html'; default_icon = [ordered]@{ '16' = 'icons/16.png'; '32' = 'icons/32.png' } }
  permissions = @('declarativeNetRequestWithHostAccess')
  host_permissions = @('<all_urls>')
  declarative_net_request = [ordered]@{ rule_resources = @([ordered]@{ id = 'pdf'; enabled = $true; path = 'rules.json' }) }
  web_accessible_resources = @([ordered]@{ resources = @('index.html', '*.js', '*.wasm', 'icons/*'); matches = @('<all_urls>') })
  content_security_policy = [ordered]@{ extension_pages = "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'" }
}
if (-not $Store) { $manifest['key'] = $key }
$utf8 = New-Object Text.UTF8Encoding $false
[IO.File]::WriteAllText((Join-Path $Out 'manifest.json'), ($manifest | ConvertTo-Json -Depth 8), $utf8)

# A tab opening a PDF goes to NadsPDF with the address in #file=. A fragment, not ?file=: the
# redirect can't URL-encode the address, and a query would split a signed URL's own &parameters
# off. Only top-level navigations: PDFs embedded in pages, and NadsPDF's own download of the file,
# are left alone. ?lite=1&lang=fr: the simplified interface (read, comment, fill in, sign, edit, OCR),
# in French.
$redirect = [ordered]@{ type = 'redirect'; redirect = [ordered]@{ regexSubstitution = "chrome-extension://$ExtensionId/index.html?lite=1&lang=fr#file=\0" } }
$pdfPath = '[^?#]*\.[pP][dD][fF]([?#].*)?$'
$rules = @(
  # Web addresses ending in .pdf, before anything is downloaded. GET only: re-requesting a form's
  # POST as a GET would lose its answer, so those stay in the browser's viewer.
  [ordered]@{
    id = 1; priority = 1; action = $redirect
    condition = [ordered]@{ regexFilter = "^https?://$pdfPath"; resourceTypes = @('main_frame'); requestMethods = @('get') }
  }
  # Files on the computer (once the user allows file access).
  [ordered]@{
    id = 2; priority = 1; action = $redirect
    condition = [ordered]@{ regexFilter = "^file://$pdfPath"; resourceTypes = @('main_frame') }
  }
  # Any other page the server says is a PDF (arxiv.org/pdf/…, viewer endpoints), judged on its
  # response headers (Chrome 128+). A PDF the server sends as a download (attachment) still downloads.
  [ordered]@{
    id = 3; priority = 1; action = $redirect
    condition = [ordered]@{
      regexFilter = '^https?://.*$'
      resourceTypes = @('main_frame')
      requestMethods = @('get')
      responseHeaders = @([ordered]@{ header = 'content-type'; values = @('application/pdf*', 'application/x-pdf*') })
      excludedResponseHeaders = @([ordered]@{ header = 'content-disposition'; values = @('attachment*') })
    }
  }
)
[IO.File]::WriteAllText((Join-Path $Out 'rules.json'), (ConvertTo-Json -InputObject $rules -Depth 8), $utf8)

$Dist = Join-Path $Root 'dist\release'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Zip = if ($Store) { Join-Path $Dist "nadspdf-$Version$(if ($Revision) { ".$Revision" })-$Store-store.zip" } else { Join-Path $Dist "nadspdf-$Version-chrome-extension.zip" }
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $Out '*') -DestinationPath $Zip
Write-Host "wrote $Out (load unpacked) and $Zip"
