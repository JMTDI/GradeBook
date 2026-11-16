; GradeBook installer for Windows (NSIS).
;
; Build it after `cargo build --release`:
;
;   makensis /DVERSION=1.0.0 /DROOT=<absolute path of the project folder> packaging/windows/installer.nsi
;
; Writes dist/GradeBook-<version>-windows-x64-setup.exe (create dist first). It installs
; for the current user only, so no administrator rights are needed. Uninstalling keeps
; the gradebook data in %APPDATA%\jmtditech\gradebook.

Unicode true
!include "MUI2.nsh"

!ifndef VERSION
    !error "Pass the version: makensis /DVERSION=x.y.z /DROOT=... installer.nsi"
!endif
; An absolute path, so nothing depends on where makensis is started from.
; Every path built from it separates its parts with backslashes. NSIS on Windows cuts a File
; path into folder and file name at the last backslash and doesn't see "/" as a separator, so
; "D:\build/target/gradebook.exe" matches no file there. makensis on Linux and macOS turns
; backslashes into "/" by itself, so this builds the same everywhere.
!ifndef ROOT
    !error "Pass the project folder: makensis /DVERSION=x.y.z /DROOT=... installer.nsi"
!endif
!ifndef EXE
    !define EXE "${ROOT}\target\release\gradebook.exe"
!endif
!ifndef OUTFILE
    !define OUTFILE "${ROOT}\dist\GradeBook-${VERSION}-windows-x64-setup.exe"
!endif

!define NAME "GradeBook"
!define PUBLISHER "JMTDI"
; The package id, which names this app's registry entries. It must match APP_ID in src/lib.rs.
!define APP_ID "com.jmtditech.gradebook"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"

Name "${NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${NAME}"
InstallDirRegKey HKCU "Software\${APP_ID}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${NAME} ${VERSION}"

!define MUI_ICON "${ROOT}\assets\icons\icon.ico"
!define MUI_UNICON "${ROOT}\assets\icons\icon.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\gradebook.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${NAME}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
    SetOutPath "$INSTDIR"
    File "/oname=gradebook.exe" "${EXE}"
    File "${ROOT}\ui\fonts\Inter-OFL.txt"
    WriteUninstaller "$INSTDIR\Uninstall.exe"

    CreateShortcut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\gradebook.exe"

    ; Remember the folder, and list the app under "Apps & features".
    WriteRegStr HKCU "Software\${APP_ID}" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${NAME}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\gradebook.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
    Delete "$INSTDIR\gradebook.exe"
    Delete "$INSTDIR\Inter-OFL.txt"
    Delete "$INSTDIR\Uninstall.exe"
    ; Never `RMDir /r`: this only removes the folder if nothing else is in it.
    RMDir "$INSTDIR"

    Delete "$SMPROGRAMS\${NAME}.lnk"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
    DeleteRegKey HKCU "Software\${APP_ID}"
SectionEnd
