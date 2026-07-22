!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\resources\installers\windows\install-service.ps1" -SupervisorExecutable "$INSTDIR\resources\service\supervisor.exe" -DataDirectory "$PROGRAMDATA\LocalAIImageFilter"'
  Pop $0
  ${If} $0 != 0
    Abort "The supervisor service could not be installed."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode = 1
    nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -Command "$s=Get-Service -Name LocalAIImageFilterSupervisor -ErrorAction Stop; Stop-Service -InputObject $s -Force -ErrorAction Stop; $s.WaitForStatus([System.ServiceProcess.ServiceControllerStatus]::Stopped,[TimeSpan]::FromSeconds(30))"'
    Pop $0
    ${If} $0 != 0
      Abort "The supervisor service could not be stopped for the signed update."
    ${EndIf}
  ${Else}
    ${IfNot} ${FileExists} "$PROGRAMDATA\LocalAIImageFilter\uninstall-authorization.token"
      Abort "Authorize uninstall from the Local AI Image Filter application first."
    ${EndIf}
    nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\resources\installers\windows\uninstall-service.ps1" -AuthorizationFile "$PROGRAMDATA\LocalAIImageFilter\uninstall-authorization.token" -FilterCtlExecutable "$INSTDIR\resources\service\filterctl.exe"'
    Pop $0
    ${If} $0 != 0
      Abort "Protected uninstall failed; network and certificate recovery must succeed before removal."
    ${EndIf}
  ${EndIf}
!macroend
