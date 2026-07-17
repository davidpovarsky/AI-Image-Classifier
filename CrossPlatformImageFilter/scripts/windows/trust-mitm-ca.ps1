param([string]$CertificatePath = "$env:USERPROFILE\.mitmproxy\mitmproxy-ca-cert.cer")
$ErrorActionPreference = "Stop"
if (-not (Test-Path -LiteralPath $CertificatePath -PathType Leaf)) {
  throw "Certificate not found: $CertificatePath. Start mitmdump once first."
}
$certificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($CertificatePath)
$thumbprint = $certificate.Thumbprint.ToUpperInvariant()
$existing = Get-ChildItem -LiteralPath Cert:\CurrentUser\Root | Where-Object Thumbprint -EQ $thumbprint
if (-not $existing) {
  Import-Certificate -FilePath $CertificatePath -CertStoreLocation Cert:\CurrentUser\Root | Out-Null
}
$stateDirectory = Join-Path $env:LOCALAPPDATA "LocalAIImageFilter"
New-Item -ItemType Directory -Path $stateDirectory -Force | Out-Null
@{ schemaVersion = 1; thumbprint = $thumbprint } | ConvertTo-Json |
  Set-Content -LiteralPath (Join-Path $stateDirectory "ca-certificate.json") -Encoding UTF8
Write-Host "Trusted mitmproxy CA for the current user (thumbprint $thumbprint)."
