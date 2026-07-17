$pidPath = Join-Path $env:LOCALAPPDATA "LocalAIImageFilter\filter-process.json"
if (-not (Test-Path -LiteralPath $pidPath -PathType Leaf)) { Write-Host "stopped"; exit 1 }
$state = Get-Content -LiteralPath $pidPath -Raw -Encoding UTF8 | ConvertFrom-Json
$process = Get-Process -Id ([int]$state.pid) -ErrorAction SilentlyContinue
if ($process -and $process.Path -eq [string]$state.executable) { Write-Host "running (PID $($state.pid))"; exit 0 }
Write-Host "stale state (PID $($state.pid) is not the saved executable)"
exit 1
