$ErrorActionPreference = "Stop"
$pidPath = Join-Path $env:LOCALAPPDATA "LocalAIImageFilter\filter-process.json"
if (-not (Test-Path -LiteralPath $pidPath -PathType Leaf)) { Write-Host "Filter is not running."; exit 0 }
$state = Get-Content -LiteralPath $pidPath -Raw -Encoding UTF8 | ConvertFrom-Json
$process = Get-Process -Id ([int]$state.pid) -ErrorAction SilentlyContinue
if ($process) {
  if ($process.Path -ne [string]$state.executable) { throw "PID was reused by another executable; refusing to stop it." }
  Stop-Process -Id $process.Id
  $process.WaitForExit(10000) | Out-Null
}
Remove-Item -LiteralPath $pidPath -Force
Write-Host "Stopped local image filter."
