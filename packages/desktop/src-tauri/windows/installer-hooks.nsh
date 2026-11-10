; 卸载 LanLink 时清掉「静默提权」计划任务，避免留下指向已删除程序的失效任务。
; 任务名与 src-tauri/src/win_task.rs 的 TASK_NAME 保持一致。
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait 'schtasks /Delete /TN LanLink /F'
!macroend
