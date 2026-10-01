param(
    [Parameter(Mandatory=$true)][int]$TargetProcessId,
    [Parameter(Mandatory=$true)][string]$ExpectedExecutable,
    [Parameter(Mandatory=$true)][string]$OutputPng
)
$ErrorActionPreference = 'Stop'
$scratchRoot = [System.IO.Path]::GetFullPath($PSScriptRoot)
$targetPath = [System.IO.Path]::GetFullPath($ExpectedExecutable)
$pngPath = [System.IO.Path]::GetFullPath($OutputPng)
if (-not $pngPath.StartsWith($scratchRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'PNG output must remain inside the screenshot scratch directory.'
}
$targetProcess = Get-Process -Id $TargetProcessId
if (-not [string]::Equals($targetProcess.Path, $targetPath, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Target process executable does not match the explicitly allowed native launcher.'
}
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SekaiWindowCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    public delegate bool EnumCallback(IntPtr window, IntPtr argument);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumCallback callback, IntPtr argument);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr destination, uint flags);
    public static IntPtr FindVisibleWindow(uint requestedProcessId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, unused) => {
            uint candidateProcessId;
            GetWindowThreadProcessId(window, out candidateProcessId);
            if (candidateProcessId != requestedProcessId) return true;
            Rect rect;
            if (IsWindowVisible(window) && GetClientRect(window, out rect) &&
                rect.Right - rect.Left >= 800 && rect.Bottom - rect.Top >= 600) {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
$window = [SekaiWindowCapture]::FindVisibleWindow([uint32]$TargetProcessId)
if ($window -eq [IntPtr]::Zero) { throw 'No visible Sekai native window found for the allowed process.' }
$rect = New-Object SekaiWindowCapture+Rect
if (-not [SekaiWindowCapture]::GetClientRect($window, [ref]$rect)) { throw 'GetClientRect failed.' }
$bitmap = New-Object System.Drawing.Bitmap ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $hdc = $graphics.GetHdc()
    try { $captured = [SekaiWindowCapture]::PrintWindow($window, $hdc, 3) }
    finally { $graphics.ReleaseHdc($hdc) }
    if (-not $captured) { throw 'PrintWindow failed; no screen-copy fallback permitted.' }
    $bitmap.Save($pngPath, [System.Drawing.Imaging.ImageFormat]::Png)
    [pscustomobject]@{
        Utc = [DateTime]::UtcNow.ToString('o')
        ProcessId = $TargetProcessId
        Executable = $targetPath
        Handle = $window.ToInt64()
        Width = $bitmap.Width
        Height = $bitmap.Height
        OutputPng = $pngPath
        Method = 'PrintWindow PW_CLIENTONLY|PW_RENDERFULLCONTENT'
        Validation = 'Pending view_image; API success alone is not visual proof'
    } | ConvertTo-Json
}
finally { $graphics.Dispose(); $bitmap.Dispose() }
