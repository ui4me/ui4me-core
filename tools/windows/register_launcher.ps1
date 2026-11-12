[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$InputJson)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$request = Get-Content -Raw -Encoding UTF8 -LiteralPath $InputJson | ConvertFrom-Json
if ($request.app_id -notmatch '^[A-Za-z][A-Za-z0-9_-]*(\.[A-Za-z][A-Za-z0-9_-]*)+$') { throw 'Invalid application identity' }
if ($request.action -eq 'location') {
    $programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    @{ path=(Join-Path $programs ($request.app_id + '.lnk')) } | ConvertTo-Json -Compress
    exit 0
}
if ($request.action -ne 'install') { throw 'Unknown launcher operation' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Ui4meShortcutIdentity {
    [StructLayout(LayoutKind.Sequential)]
    public struct PropertyKey { public Guid format; public uint id; }
    [StructLayout(LayoutKind.Explicit, Size=24)]
    public struct PropVariant {
        [FieldOffset(0)] public ushort type;
        [FieldOffset(8)] public IntPtr pointer;
    }
    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore {
        [PreserveSig] int GetCount(out uint count);
        [PreserveSig] int GetAt(uint index, out PropertyKey key);
        [PreserveSig] int GetValue(ref PropertyKey key, out PropVariant value);
        [PreserveSig] int SetValue(ref PropertyKey key, ref PropVariant value);
        [PreserveSig] int Commit();
    }
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, PreserveSig=true)]
    private static extern int SHGetPropertyStoreFromParsingName(string path, IntPtr context, uint flags,
        ref Guid iid, [MarshalAs(UnmanagedType.Interface)] out IPropertyStore store);
    [DllImport("ole32.dll", PreserveSig=true)]
    private static extern int PropVariantClear(ref PropVariant value);
    public static void SetAndVerify(string path, string appId) {
        var iid = typeof(IPropertyStore).GUID;
        IPropertyStore store;
        Marshal.ThrowExceptionForHR(SHGetPropertyStoreFromParsingName(path, IntPtr.Zero, 2, ref iid, out store));
        var key = new PropertyKey { format = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), id = 5 };
        var value = new PropVariant { type = 31, pointer = Marshal.StringToCoTaskMemUni(appId) };
        try {
            Marshal.ThrowExceptionForHR(store.SetValue(ref key, ref value));
            Marshal.ThrowExceptionForHR(store.Commit());
            PropVariant actual;
            Marshal.ThrowExceptionForHR(store.GetValue(ref key, out actual));
            try {
                if (actual.type != 31 || Marshal.PtrToStringUni(actual.pointer) != appId)
                    throw new InvalidOperationException("Shortcut identity verification failed");
            } finally { PropVariantClear(ref actual); }
        } finally {
            Marshal.FreeCoTaskMem(value.pointer);
            Marshal.FinalReleaseComObject(store);
        }
    }
}
'@
$shell = New-Object -ComObject WScript.Shell
$shortcut = $null
try {
    $shortcut = $shell.CreateShortcut([string]$request.path)
    $shortcut.TargetPath = [string]$request.host
    $shortcut.Arguments = [string]$request.arguments
    $shortcut.WorkingDirectory = [string]$request.cwd
    $shortcut.Description = 'UI4ME managed launcher: ' + [string]$request.app_id
    $shortcut.IconLocation = ([string]$request.icon) + ',0'
    $shortcut.Save()
} finally {
    if ($shortcut) { [Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut) | Out-Null }
    [Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) | Out-Null
}
[Ui4meShortcutIdentity]::SetAndVerify([string]$request.path, [string]$request.app_id)
@{ status='installed'; path=$request.path; app_id=$request.app_id; identity_verified=$true } | ConvertTo-Json -Compress
