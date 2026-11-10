; 卸载 LanLink 时清掉「静默提权」计划任务，避免留下指向已删除程序的失效任务。
; 任务名与 src-tauri/src/win_task.rs 的 TASK_NAME 保持一致。
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait 'schtasks /Delete /TN LanLink /F'
!macroend

; 覆盖安装前清掉旧版本留下的 WinDivert 内核驱动。
; 只有 .sys 不再分发：写驱动文件会被安全软件/驱动保护拦下来，让 NSIS 覆盖时报
; "Error opening file for writing"。已经装过旧版的机器需要在这里删除，否则会一直残留。
; 注意：Packet.dll 现在是 easytier-core.exe 的静态导入依赖，必须保留，
; 它会由本次安装正常覆盖写入，不能在这里删除。
!macro NSIS_HOOK_PREINSTALL
  ClearErrors
  SetFileAttributes "$INSTDIR\core\windows-x64\WinDivert64.sys" NORMAL
  Delete "$INSTDIR\core\windows-x64\WinDivert64.sys"
!macroend
