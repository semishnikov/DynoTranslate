!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$INSTDIR\resources\DirectML.dll"
    CopyFiles /SILENT "$INSTDIR\resources\DirectML.dll" "$INSTDIR\DirectML.dll"
  ${EndIf}
!macroend
