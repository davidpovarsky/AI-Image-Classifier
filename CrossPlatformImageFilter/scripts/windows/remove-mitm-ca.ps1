$ErrorActionPreference = "Stop"
$statePath = Join-Path $env:LOCALAPPDATA "LocalAIImageFilter\ca-certificate.json"
if (-not (Test-Path -LiteralPath $statePath -PathType Leaf)) {
  throw "Saved CA thumbprint was not found: $statePath. Refusing name-based deletion."
}
$state = Get-Content -LiteralPath $statePath -Raw -Encoding UTF8 | ConvertFrom-Json
$thumbprint = [string]$state.thumbprint
if ($thumbprint -notmatch '^[0-9A-Fa-f]{40,64}$') { throw "Saved CA thumbprint is invalid." }
$certificate = Get-ChildItem -LiteralPath Cert:\CurrentUser\Root | Where-Object Thumbprint -EQ $thumbprint
if ($certificate) { $certificate | Remove-Item -Force }
Remove-Item -LiteralPath $statePath -Force
Write-Host "Removed only the saved mitmproxy CA thumbprint $thumbprint from CurrentUser Root."
