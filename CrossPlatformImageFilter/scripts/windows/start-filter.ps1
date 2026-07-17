param([string]$Config = "")
$ErrorActionPreference = "Stop"
$stateDirectory = Join-Path $env:LOCALAPPDATA "LocalAIImageFilter"
$pidPath = Join-Path $stateDirectory "filter-process.json"
New-Item -ItemType Directory -Path $stateDirectory -Force | Out-Null
if (Test-Path -LiteralPath $pidPath -PathType Leaf) {
  $state = Get-Content -LiteralPath $pidPath -Raw -Encoding UTF8 | ConvertFrom-Json
  if (Get-Process -Id ([int]$state.pid) -ErrorAction SilentlyContinue) {
    Write-Host "Local image filter is already running (PID $($state.pid))."
    exit 0
  }
  Remove-Item -LiteralPath $pidPath -Force
}
$executable = (Get-Command local-image-filter.exe -ErrorAction Stop).Source
$arguments = @("run")
if ($Config) { $arguments += @("--config", (Resolve-Path -LiteralPath $Config).Path) }
$process = Start-Process -FilePath $executable -ArgumentList $arguments -PassThru -WindowStyle Hidden `
  -RedirectStandardOutput (Join-Path $stateDirectory "filter.stdout.log") `
  -RedirectStandardError (Join-Path $stateDirectory "filter.stderr.log")
@{ schemaVersion = 1; pid = $process.Id; executable = $executable; startedAt = (Get-Date).ToUniversalTime().ToString("o") } |
  ConvertTo-Json | Set-Content -LiteralPath $pidPath -Encoding UTF8
Write-Host "Started local image filter (PID $($process.Id))."
