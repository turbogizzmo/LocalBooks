; Installer hooks for the Windows (NSIS) installer, see bundle.windows.nsis.installerHooks.
; The release check's installer upgrade step (release-check.yml) proves this on every release.
;
; Until 0.16 the app was called LocalBooks. Tauri's installer keys the install folder, the
; uninstall entry and the shortcuts on the product name, so the first Tillhouse installer finds
; no earlier install and puts Tillhouse in its own folder. This hook then removes the LocalBooks
; copy it replaces: its program files, shortcuts and uninstall entry. Company files, settings,
; backups and saved passwords are not touched: they are keyed on the bundle identifier
; (com.localbooks.desktop), which did not change.

!include LogicLib.nsh

!macro NSIS_HOOK_POSTINSTALL
  ; The old install's folder, as its installer recorded it ("Software\<manufacturer>\<product>").
  ReadRegStr $R0 SHCTX "Software\localbooks\LocalBooks" ""
  ${If} $R0 != ""
  ${AndIf} $R0 != $INSTDIR
  ${AndIf} ${FileExists} "$R0\localbooks.exe"
    ; Only the files the LocalBooks installer put there, never the whole folder.
    Delete "$R0\localbooks.exe"
    Delete "$R0\uninstall.exe"
    Delete "$R0\THIRD-PARTY-NOTICES.txt"
    RMDir "$R0"
  ${EndIf}
  ; Replace the LocalBooks shortcuts with Tillhouse ones. An update runs this installer in update
  ; mode, which only refreshes shortcuts that already exist under the new name, so without this the
  ; app would drop out of the Start menu (caught by the release check of 0.16.0). Only where the old
  ; shortcut was, so a later update never brings back a shortcut someone deleted on purpose.
  ${If} ${FileExists} "$SMPROGRAMS\LocalBooks.lnk"
    ${IfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      CreateShortCut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
    Delete "$SMPROGRAMS\LocalBooks.lnk"
  ${EndIf}
  ${If} ${FileExists} "$DESKTOP\LocalBooks.lnk"
    ${IfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
      CreateShortCut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
    Delete "$DESKTOP\LocalBooks.lnk"
  ${EndIf}
  DeleteRegKey SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalBooks"
  DeleteRegKey SHCTX "Software\localbooks\LocalBooks"
!macroend
