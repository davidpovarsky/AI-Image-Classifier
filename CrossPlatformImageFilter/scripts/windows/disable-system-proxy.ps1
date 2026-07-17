$ErrorActionPreference = "Stop"
$registryPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
$backupPath = Join-Path $env:LOCALAPPDATA "LocalAIImageFilter\proxy-backup.json"
if (-not (Test-Path -LiteralPath $backupPath -PathType Leaf)) {
  throw "Proxy backup was not found: $backupPath. Refusing to guess the previous state."
}
$backup = Get-Content -LiteralPath $backupPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($backup.schemaVersion -ne 1) { throw "Unsupported proxy backup schema." }
foreach ($name in @("ProxyEnable", "ProxyServer", "ProxyOverride", "AutoConfigURL", "AutoDetect")) {
  $entry = $backup.values.$name
  if ($entry.exists) {
    Set-ItemProperty -LiteralPath $registryPath -Name $name -Value $entry.value
  } else {
    Remove-ItemProperty -LiteralPath $registryPath -Name $name -ErrorAction SilentlyContinue
  }
}
Remove-Item -LiteralPath $backupPath -Force
Write-Host "Restored the previous Windows user proxy configuration."
