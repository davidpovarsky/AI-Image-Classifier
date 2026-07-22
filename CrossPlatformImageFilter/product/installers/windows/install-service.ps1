[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)][ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })][string]$SupervisorExecutable,
    [Parameter(Mandatory)][string]$DataDirectory,
    [string]$ServiceName = 'LocalAIImageFilterSupervisor'
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = (Resolve-Path -LiteralPath $SupervisorExecutable).Path
$fullDataPath = [IO.Path]::GetFullPath($DataDirectory)
if (-not (Test-Path -LiteralPath $fullDataPath -PathType Container)) {
    New-Item -ItemType Directory -Path $fullDataPath -Force | Out-Null
}
$resolvedData = (Resolve-Path -LiteralPath $fullDataPath).Path
if ($PSCmdlet.ShouldProcess($ServiceName, 'Install and start supervisor service')) {
    $existingService = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    $createdService = -not $existingService
    if ($existingService) {
        if ($existingService.Status -ne 'Stopped') {
            Stop-Service -Name $ServiceName -Force
            $existingService.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
        }
        & sc.exe config $ServiceName binPath= ('"{0}" --service' -f $resolvedExecutable) | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Failed to update the supervisor service path.' }
    } else {
        New-Service -Name $ServiceName -BinaryPathName ('"{0}" --service' -f $resolvedExecutable) `
            -DisplayName 'Local AI Image Filter Supervisor' -StartupType Automatic
    }
    try {
        & sc.exe config $ServiceName start= delayed-auto obj= 'LocalSystem' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Failed to configure delayed service startup.' }
        & sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/15000/none/0 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Failed to configure bounded service recovery.' }
        $serviceSddl = 'D:(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWLOCRRC;;;AU)'
        & sc.exe sdset $ServiceName $serviceSddl | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Failed to configure the service DACL.' }
        & icacls.exe $resolvedData /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Failed to protect the service data directory.' }
        Start-Service -Name $ServiceName
        (Get-Service -Name $ServiceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
    } catch {
        Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue
        if ($createdService) {
            & sc.exe delete $ServiceName | Out-Null
        }
        throw
    }
}
