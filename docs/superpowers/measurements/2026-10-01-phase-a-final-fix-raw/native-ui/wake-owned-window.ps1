param(
    [Parameter(Mandatory=$true)][int]$TargetProcessId,
    [Parameter(Mandatory=$true)][string]$ExpectedExecutable,
    [Parameter(Mandatory=$true)][string]$Command,
    [long]$WindowHandle = 0
)
$ErrorActionPreference = 'Stop'
$scratchRoot = [System.IO.Path]::GetFullPath($PSScriptRoot)
$targetPath = [System.IO.Path]::GetFullPath($ExpectedExecutable)
if (-not $targetPath.StartsWith($scratchRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'The allowed executable must remain inside screenshot scratch.'
}
$targetProcess = Get-Process -Id $TargetProcessId
if (-not [string]::Equals($targetProcess.Path, $targetPath, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Target process executable does not match the allowed native launcher.'
}
if ($Command -notmatch '^(click |drag |wheel |text |ctrl-a$|enter$|escape$|ctrl-alt-f$|close$)' -or $Command.Contains("`n") -or $Command.Contains("`r")) {
    throw 'Use one supported native-driver command: click, drag, wheel, text, ctrl-a, enter, escape, ctrl-alt-f, or close.'
}
if (-not ('SekaiOwnedWindowWake' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SekaiOwnedWindowWake {
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool InvalidateRect(IntPtr window, IntPtr rect, bool erase);
}
'@
}
if ($WindowHandle -eq 0) {
    $targetProcess.Refresh()
    $window = $targetProcess.MainWindowHandle
} else {
    $window = [IntPtr]::new($WindowHandle)
}
[uint32]$ownerProcessId = 0
$windowThread = [SekaiOwnedWindowWake]::GetWindowThreadProcessId($window, [ref]$ownerProcessId)
if ($windowThread -eq 0 -or $ownerProcessId -ne [uint32]$TargetProcessId -or -not [SekaiOwnedWindowWake]::IsWindowVisible($window)) {
    throw 'Window is not a visible window owned by the allowed process.'
}
$commandPath = Join-Path $scratchRoot 'ui-command.txt'
$pendingPath = Join-Path $scratchRoot 'ui-command.pending.txt'
if ((Test-Path -LiteralPath $commandPath) -or (Test-Path -LiteralPath $pendingPath)) {
    throw 'A previous native-driver command remains pending; do not overwrite it.'
}
[System.IO.File]::WriteAllText($pendingPath, $Command, [System.Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $pendingPath -Destination $commandPath
[uint32]$confirmedOwner = 0
if ([SekaiOwnedWindowWake]::GetWindowThreadProcessId($window, [ref]$confirmedOwner) -eq 0 -or $confirmedOwner -ne [uint32]$TargetProcessId) {
    throw 'Window ownership changed before wake; the pending command was retained.'
}
if (-not [SekaiOwnedWindowWake]::InvalidateRect($window, [IntPtr]::Zero, $false)) {
    throw ('InvalidateRect failed; pending command retained. Win32 error: ' + [System.Runtime.InteropServices.Marshal]::GetLastWin32Error())
}
[pscustomobject]@{
    Utc = [DateTime]::UtcNow.ToString('o')
    ProcessId = $TargetProcessId
    Handle = $window.ToInt64()
    Executable = $targetPath
    CommandFile = $commandPath
    Wake = 'InvalidateRect on verified owned HWND; no focus change'
    Validation = 'Wake requested; command consumption and repaint require native runtime verification'
} | ConvertTo-Json
