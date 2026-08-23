param(
    [Parameter(Mandatory = $true)]
    [string]$Executable
)

$resolved = (Resolve-Path $Executable -ErrorAction Stop).Path

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class VibezPeResources
{
    private const uint LoadLibraryAsDataFile = 0x00000002;
    private static readonly IntPtr ApplicationIconId = new IntPtr(1);
    private static readonly IntPtr GroupIconType = new IntPtr(14);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibraryExW(string fileName, IntPtr file, uint flags);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr FindResourceW(IntPtr module, IntPtr name, IntPtr type);

    [DllImport("kernel32.dll")]
    private static extern bool FreeLibrary(IntPtr module);

    public static bool HasApplicationIcon(string path)
    {
        IntPtr module = LoadLibraryExW(path, IntPtr.Zero, LoadLibraryAsDataFile);
        if (module == IntPtr.Zero)
        {
            throw new InvalidOperationException(
                "Could not load executable resources. Win32 error " + Marshal.GetLastWin32Error());
        }

        try
        {
            return FindResourceW(module, ApplicationIconId, GroupIconType) != IntPtr.Zero;
        }
        finally
        {
            FreeLibrary(module);
        }
    }
}
"@

if (-not [VibezPeResources]::HasApplicationIcon($resolved)) {
    throw "The vibez executable has no RT_GROUP_ICON resource with application icon ID 1"
}

$versionInfo = (Get-Item $resolved).VersionInfo
if ([string]::IsNullOrWhiteSpace($versionInfo.ProductName)) {
    throw "The executable has no ProductName version resource"
}

Write-Host "Verified embedded vibez application icon and version resources in $resolved"
