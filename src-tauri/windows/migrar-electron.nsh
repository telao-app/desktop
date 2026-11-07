; Quem tinha o app Electron (0.1.x) e atualiza por ele recebe este instalador: tira o Electron antigo para não ficar
; com dois Telão instalados e, como o atualizador do Electron espera, abre o app de novo no fim.
!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$LOCALAPPDATA\Programs\telao-desktop\Uninstall Telao.exe" 0 +2
    ExecWait '"$LOCALAPPDATA\Programs\telao-desktop\Uninstall Telao.exe" /S'
  ${GetParameters} $R0
  ${GetOptions} $R0 "--updated" $R1
  IfErrors +2 0
    Exec '"$INSTDIR\Telao.exe"'
!macroend
