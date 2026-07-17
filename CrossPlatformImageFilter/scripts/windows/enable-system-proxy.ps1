param(
  [ValidateNotNullOrEmpty()][string]$HostAddress = "127.0.0.1",
  [ValidateRange(1, 65535)][int]$Port = 8080
)
$ErrorActionPreference = "Stop"
$registryPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
$stateDirectory = Join-Path $env:LOCALAPPDATA "LocalAIImageFilter"
$backupPath = Join-Path $stateDirectory "proxy-backup.json"
New-Item -ItemType Directory -Path $stateDirectory -Force | Out-Null

if (-not (Test-Path -LiteralPath $backupPath -PathType Leaf)) {
  $properties = Get-ItemProperty -LiteralPath $registryPath
  $names = @("ProxyEnable", "ProxyServer", "ProxyOverride", "AutoConfigURL", "AutoDetect")
  $values = @{}
  foreach ($name in $names) {
    $exists = $null -ne $properties.PSObject.Properties[$name]
    $values[$name] = @{ exists = $exists; value = if ($exists) { $properties.$name } else { $null } }
  }
  @{ schemaVersion = 1; capturedAt = (Get-Date).ToUniversalTime().ToString("o"); values = $values } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $backupPath -Encoding UTF8
}

Set-ItemProperty -LiteralPath $registryPath -Name ProxyEnable -Type DWord -Value 1
Set-ItemProperty -LiteralPath $registryPath -Name ProxyServer -Type String -Value "http=$HostAddress`:$Port;https=$HostAddress`:$Port"
Set-ItemProperty -LiteralPath $registryPath -Name ProxyOverride -Type String -Value "<local>;localhost;127.0.0.1;::1"
Write-Host "Windows user proxy enabled at $HostAddress`:$Port. Previous settings: $backupPath"
