; Form1099Keep's Windows setup, patterned on CalcKeep's installer/calckeep.nsi and
; MoneyKeep's installer/moneykeep.nsi.
;
; Built by `cargo xtask installer`, which first runs `cargo xtask release` and then
; calls makensis with:
;   -DVERSION=0.1.0-alpha.1  the version (Cargo.toml's [package] version)
;   -DVERSION_QUAD=0.1.0.1   the same as four numbers (Windows file versions; the
;                            pre-release number goes last)
;   -DSOURCE_DIR=<folder>    dist\Form1099Keep-<version>: form1099keep.exe,
;                            form1099keep-cli.exe, form1099keep.ico, LICENSE-MIT,
;                            LICENSE-APACHE, NOTICE, README.md, THIRD-PARTY-LICENSES.txt
;                            and licence.txt
;                            (NOTICE + the MIT and Apache-2.0 texts with Windows line ends)
;   -DOUT_FILE=<file>        dist\form1099keep-<version>-windows-x64-setup.exe (the name
;                            MoneyKeep Launcher installs; SHA256SUMS.txt is written beside it)
;
; Written from the NSIS documentation (https://nsis.sourceforge.io/Docs/): the script
; language, Modern UI 2, LogicLib, x64.nsh and FileFunc.nsh, all shipped with NSIS.
;
; What it does:
; - per-machine install in C:\Program Files\Form1099Keep, with administrator permission;
; - pages: welcome (says which version is installed or updated), licence (MIT OR Apache-2.0),
;   options (Start menu shortcut on, Desktop shortcut off), folder (skipped when
;   updating in place), progress, finish;
; - registers in Windows "Installed apps" with name, version, publisher, icon, size and
;   the uninstaller;
; - asks to close Form1099Keep when it is open;
; - the uninstaller removes the program files, shortcuts and registration, and asks
;   whether to remove the user's settings (the recent books list, in
;   %APPDATA%\Form1099Keep\config\settings.json; default No). Books are MoneyKeep book
;   files the user keeps wherever they choose: they are never touched.
; - security-review-1 L8: the settings are removed only when neither the
;   Form1099Keep folder nor its config folder is a link or junction, in the %APPDATA%
;   of the user who started the uninstall: Installed apps runs the uninstaller through
;   a small cmd.exe stub that passes that user's %APPDATA%, captured before the
;   permission prompt, so an administrator who approves it doesn't make it look in the
;   administrator's own folder. Otherwise they are kept and the details say why.
;
; Silent mode (/S, as MoneyKeep Launcher runs it):
; - no pages and no questions: the default options (Start menu shortcut on, Desktop
;   shortcut off; an update leaves the shortcuts that are there alone), the folder
;   from /D= or, when updating, the installed folder;
; - Form1099Keep open: it is asked to close (as its window's close button does; books
;   are saved as you work) and Setup waits up to 30 s, then stops with exit code 2
;   ("Close Form1099Keep and try again");
; - exit code 0 when done, 3 when anything else stopped Setup;
; - the silent uninstall (QuietUninstallString) removes the program and never the
;   user's settings (the question's default, No, is taken without asking).

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma

!macro REQUIRE name
  !ifndef ${name}
    !error "${name} is not set: run cargo xtask installer (it passes VERSION, VERSION_QUAD, SOURCE_DIR and OUT_FILE)."
  !endif
!macroend
!insertmacro REQUIRE VERSION
!insertmacro REQUIRE VERSION_QUAD
!insertmacro REQUIRE SOURCE_DIR
!insertmacro REQUIRE OUT_FILE

; ----- names shared with Form1099Keep's code and MoneyKeep Launcher ------------------------

!define PRODUCT "Form1099Keep"
!define PUBLISHER "MoneyKeep Project"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Form1099Keep"
!define APP_EXE "form1099keep.exe"
!define CLI_EXE "form1099keep-cli.exe"
!define ICON_FILE "form1099keep.ico"
; Settings (src/main.rs config_path: the directories crate's config folder for
; "Form1099Keep", %APPDATA%\Form1099Keep\config on Windows).
!define DATA_DIR "Form1099Keep"
!define SETTINGS "config\settings.json"
; Silent mode's exit codes besides 0 (MoneyKeep Launcher reads them).
!define EXIT_APP_OPEN 2
!define EXIT_FAILED 3

; ----- the setup program --------------------------------------------------------------

Name "${PRODUCT}"
OutFile "${OUT_FILE}"
InstallDir "$PROGRAMFILES64\${PRODUCT}"
RequestExecutionLevel admin
BrandingText "${PRODUCT} ${VERSION}"

VIProductVersion "${VERSION_QUAD}"
VIFileVersion "${VERSION_QUAD}"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${PRODUCT} Setup"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 MoneyKeep Project contributors (MIT OR Apache-2.0)"

!include MUI2.nsh
!include LogicLib.nsh
!include Sections.nsh
!include x64.nsh
!include FileFunc.nsh

Var OldVersion      ; the installed version, "" when none
Var WelcomeText     ; the first page's text
Var Haystack        ; Contains: is $Needle in $Haystack? -> $Found (1 or 0)
Var Needle
Var Found
Var OrigAppData     ; the %APPDATA% of the user who started the uninstall (L8)

; The app icon is the setup's and the uninstaller's icon.
!define MUI_ICON "${SOURCE_DIR}\${ICON_FILE}"
!define MUI_UNICON "${SOURCE_DIR}\${ICON_FILE}"
!define MUI_ABORTWARNING

; ----- pages --------------------------------------------------------------------------

!define MUI_WELCOMEPAGE_TITLE "${PRODUCT} ${VERSION}"
!define MUI_WELCOMEPAGE_TEXT "$WelcomeText"
!insertmacro MUI_PAGE_WELCOME

; The licence (MIT OR Apache-2.0); nothing to agree to for using it, so the button is Next.
!define MUI_LICENSEPAGE_TEXT_TOP "Form1099Keep is free software under the MIT or Apache-2.0 licence, at your option."
!define MUI_LICENSEPAGE_TEXT_BOTTOM "The licence is installed with Form1099Keep (LICENSE-MIT, LICENSE-APACHE, NOTICE)."
!define MUI_LICENSEPAGE_BUTTON "$(^NextBtn)"
!insertmacro MUI_PAGE_LICENSE "${SOURCE_DIR}\licence.txt"

; Start menu shortcut (on), Desktop shortcut (off). Leaving the page checks that
; Form1099Keep is closed.
!define MUI_COMPONENTSPAGE_NODESC
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE CheckAppClosed
!insertmacro MUI_PAGE_COMPONENTS

; The folder; skipped when updating (the update goes where Form1099Keep is).
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipFolderOnUpdate
!insertmacro MUI_PAGE_DIRECTORY

!insertmacro MUI_PAGE_INSTFILES

; No "Open Form1099Keep" box: Setup runs as administrator, and Form1099Keep should not.
!define MUI_FINISHPAGE_TEXT "${PRODUCT} ${VERSION} is installed. Open it from the Start menu."
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ----- helpers (each for Setup and, with the un. prefix, for the uninstaller) ---------

; $Found = 1 when $Needle occurs in $Haystack (letter case ignored), else 0.
!macro CONTAINS_FN un
Function ${un}Contains
  StrCpy $Found 0
  StrLen $R1 $Needle
  StrCpy $R2 0
  ${Do}
    StrCpy $R3 $Haystack $R1 $R2
    ${If} $R3 == ""
      ${Break}
    ${EndIf}
    ${If} $R3 == $Needle
      StrCpy $Found 1
      ${Break}
    ${EndIf}
    IntOp $R2 $R2 + 1
  ${Loop}
FunctionEnd
!macroend
!insertmacro CONTAINS_FN ""
!insertmacro CONTAINS_FN "un."

; Form1099Keep open? Ask to close it: Retry checks again, Cancel stops here (on a page:
; stays on it; while installing or uninstalling: stops).
!macro APP_CLOSED_FN un
Function ${un}CheckAppClosed
  ${Do}
    nsExec::ExecToStack `"$SYSDIR\tasklist.exe" /FI "IMAGENAME eq ${APP_EXE}" /FO CSV /NH`
    Pop $0 ; exit code
    Pop $Haystack
    StrCpy $Needle "${APP_EXE}"
    Call ${un}Contains
    ${If} $Found != 1
      ${Break}
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Form1099Keep is open. Close it (your books are saved as you work), then press Retry." /SD IDCANCEL IDRETRY retry
    ${If} ${Silent}
      SetErrorLevel ${EXIT_APP_OPEN}
    ${EndIf}
    Abort
    retry:
  ${Loop}
FunctionEnd
!macroend
!insertmacro APP_CLOSED_FN ""
!insertmacro APP_CLOSED_FN "un."

; Silent mode: Form1099Keep open? Ask it to close (taskkill without /F sends its window
; the close request its close button sends) and wait up to 30 s; still open: stop
; with exit code 2.
!macro SILENT_CLOSE_FN un
Function ${un}SilentCloseApp
  StrCpy $R5 0
  ${Do}
    nsExec::ExecToStack `"$SYSDIR\tasklist.exe" /FI "IMAGENAME eq ${APP_EXE}" /FO CSV /NH`
    Pop $0 ; exit code
    Pop $Haystack
    StrCpy $Needle "${APP_EXE}"
    Call ${un}Contains
    ${If} $Found != 1
      ${Break}
    ${EndIf}
    ${If} $R5 == 0
      nsExec::ExecToStack `"$SYSDIR\taskkill.exe" /IM ${APP_EXE}`
      Pop $0
      Pop $0
    ${EndIf}
    ${If} $R5 >= 30
      SetErrorLevel ${EXIT_APP_OPEN}
      Quit
    ${EndIf}
    Sleep 1000
    IntOp $R5 $R5 + 1
  ${Loop}
FunctionEnd
!macroend
!insertmacro SILENT_CLOSE_FN ""
!insertmacro SILENT_CLOSE_FN "un."

; ----- Setup --------------------------------------------------------------------------

Function .onInit
  ; Silent: anything that stops Setup before it is done exits 3 (the last section sets 0).
  ${If} ${Silent}
    SetErrorLevel ${EXIT_FAILED}
  ${EndIf}
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "Form1099Keep needs 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  ; The 64-bit registry and System32 (tasklist.exe, taskkill.exe).
  SetRegView 64
  ${DisableX64FSRedirection}
  ReadRegStr $OldVersion HKLM "${UNINSTALL_KEY}" "DisplayVersion"
  ReadRegStr $0 HKLM "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
  StrCpy $1 "Setup needs administrator permission: Form1099Keep is installed for all users."
  ${If} $OldVersion == ""
    StrCpy $WelcomeText "Setup will install ${PRODUCT} ${VERSION} on this computer.$\r$\n$\r$\n$1"
  ${ElseIf} $OldVersion == "${VERSION}"
    StrCpy $WelcomeText "${PRODUCT} ${VERSION} is already installed; Setup will install it again. Your books and settings are kept.$\r$\n$\r$\n$1"
  ${Else}
    StrCpy $WelcomeText "${PRODUCT} $OldVersion will be updated to ${VERSION}. Your books and settings are kept.$\r$\n$\r$\n$1"
  ${EndIf}
  ${If} ${Silent}
    Call SilentCloseApp
  ${EndIf}
FunctionEnd

Function SkipFolderOnUpdate
  ${If} $OldVersion != ""
    Abort ; skips the page
  ${EndIf}
FunctionEnd

Section "Form1099Keep (required)" SecMain
  SectionIn RO
  Call CheckAppClosed

  SetOutPath "$INSTDIR"
  File "${SOURCE_DIR}\${APP_EXE}"
  File "${SOURCE_DIR}\${CLI_EXE}"
  File "${SOURCE_DIR}\${ICON_FILE}"
  File "${SOURCE_DIR}\LICENSE-MIT"
  File "${SOURCE_DIR}\LICENSE-APACHE"
  File "${SOURCE_DIR}\NOTICE"
  File "${SOURCE_DIR}\README.md"
  ; Every crate compiled into the programs with its licence (security-review-1 M8).
  File "${SOURCE_DIR}\THIRD-PARTY-LICENSES.txt"

  ; Windows "Installed apps".
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${ICON_FILE}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  ; L8: cmd.exe, started by Installed apps as the user, expands that user's %APPDATA%
  ; and hands it to the uninstaller before Windows asks for administrator permission.
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$SYSDIR\cmd.exe" /d /c start "" "$INSTDIR\uninstall.exe" /APPDATA="%APPDATA%"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Start menu shortcut" SecStartMenu
  SetShellVarContext all
  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${ICON_FILE}" 0
SectionEnd

Section /o "Desktop shortcut" SecDesktop
  SetShellVarContext all
  CreateShortcut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${ICON_FILE}" 0
SectionEnd

; An update with a shortcut turned off removes the one an earlier version made. A
; silent update chose nothing, so it leaves the shortcuts that are there alone.
Section "-Shortcuts turned off"
  ${IfNot} ${Silent}
    SetShellVarContext all
    ${IfNot} ${SectionIsSelected} ${SecStartMenu}
      Delete "$SMPROGRAMS\${PRODUCT}.lnk"
    ${EndIf}
    ${IfNot} ${SectionIsSelected} ${SecDesktop}
      Delete "$DESKTOP\${PRODUCT}.lnk"
    ${EndIf}
  ${EndIf}
SectionEnd

; The last section: Setup got this far, so it succeeded (silent mode's exit code 0).
Section "-Done"
  SetErrorLevel 0
SectionEnd

; ----- the uninstaller ----------------------------------------------------------------

Function un.onInit
  ${If} ${Silent}
    SetErrorLevel ${EXIT_FAILED}
  ${EndIf}
  SetRegView 64
  ${DisableX64FSRedirection}
  ${If} ${Silent}
    Call un.SilentCloseApp
  ${EndIf}
  ; L8: the original user's %APPDATA% (from the stub), when it is a real folder; else
  ; this account's.
  SetShellVarContext current
  StrCpy $OrigAppData $APPDATA
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/APPDATA=" $R1
  ${IfNot} ${Errors}
    StrCpy $R2 $R1 1
    ${If} $R2 == '"'
      StrCpy $R1 $R1 "" 1
      StrCpy $R1 $R1 -1
    ${EndIf}
    ${GetFileAttributes} "$R1" "REPARSE_POINT" $R2
    ${If} $R1 != ""
    ${AndIf} ${FileExists} "$R1\*.*"
    ${AndIf} $R2 != 1
      StrCpy $OrigAppData $R1
    ${EndIf}
  ${EndIf}
FunctionEnd

Section "Uninstall"
  Call un.CheckAppClosed

  ; Only the files Setup installed (books are never here).
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${CLI_EXE}"
  Delete "$INSTDIR\${ICON_FILE}"
  Delete "$INSTDIR\LICENSE-MIT"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\NOTICE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\THIRD-PARTY-LICENSES.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  SetShellVarContext all
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegKey HKLM "${UNINSTALL_KEY}"

  ; Settings (the recent books list): kept unless the user says Yes. Only the settings
  ; file is deleted, and its folders only when that leaves them empty; books are never
  ; touched.
  ; L8: in the %APPDATA% of the user who started the uninstall, and only through real
  ; folders (no link or junction on the way).
  StrCpy $0 "$OrigAppData\${DATA_DIR}"
  ${If} ${FileExists} "$0\${SETTINGS}"
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Also remove your Form1099Keep settings (the list of recent books)? Your books are not touched." /SD IDNO IDNO keep_data
    ${GetFileAttributes} "$0" "REPARSE_POINT" $1
    ${GetFileAttributes} "$0\config" "REPARSE_POINT" $2
    ${If} $1 == 1
    ${OrIf} $2 == 1
      DetailPrint "Kept $0: it or its config folder is a link or junction."
    ${Else}
      DetailPrint "Removing Form1099Keep's settings in $0"
      Delete "$0\${SETTINGS}"
      RMDir "$0\config"
      RMDir "$0"
    ${EndIf}
    keep_data:
  ${EndIf}
  SetErrorLevel 0
SectionEnd
