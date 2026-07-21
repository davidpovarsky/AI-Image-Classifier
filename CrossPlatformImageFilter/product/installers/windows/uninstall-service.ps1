[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)][string]$AuthorizationFile,
    [Parameter(Mandatory)][ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })][string]$FilterCtlExecutable,
    [string]$ServiceName = 'LocalAIImageFilterSupervisor'
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $AuthorizationFile -PathType Leaf)) {
    throw 'A service-issued uninstall authorization file is required.'
}
$resolvedFilterCtl = (Resolve-Path -LiteralPath $FilterCtlExecutable).Path
& $resolvedFilterCtl prepare-uninstall --authorization-file $AuthorizationFile | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'The supervisor rejected uninstall authorization or network recovery.' }
if ($PSCmdlet.ShouldProcess($ServiceName, 'Stop and unregister supervisor service')) {
    $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if ($service) {
        Stop-Service -Name $ServiceName -Force
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
        & sc.exe delete $ServiceName | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Failed to delete service $ServiceName." }
    }
}
