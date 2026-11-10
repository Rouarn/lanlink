; 卸载 LanLink 时清掉「静默提权」计划任务，避免留下指向已删除程序的失效任务。
; 任务名与 src-tauri/src/win_task.rs 的 TASK_NAME 保持一致。
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait 'schtasks /Delete /TN LanLink /F'
!macroend

; 覆盖安装前清掉旧版本留下的 WinDivert 驱动。
; 0.1.0 早期安装包把 WinDivert64.sys / Packet.dll 一起分发过，现在不再打包：
; 既不是运行必需（没有启用 EasyTier 的 --enable-udp-broadcast-relay），
; 又会因为内核驱动被安全软件拦截，让 NSIS 覆盖时报 "Error opening file for writing"。
; 已经装过旧版的机器需要在这里删除，否则旧文件会一直残留。
!macro NSIS_HOOK_PREINSTALL
  ClearErrors
  SetFileAttributes "$INSTDIR\core\windows-x64\WinDivert64.sys" NORMAL
  Delete "$INSTDIR\core\windows-x64\WinDivert64.sys"
  SetFileAttributes "$INSTDIR\core\windows-x64\Packet.dll" NORMAL
  Delete "$INSTDIR\core\windows-x64\Packet.dll"
!macroend
