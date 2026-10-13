[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$statePath = Join-Path $env:USERPROFILE '.rightclick-utilities\install-state.json'
$state = if (Test-Path -LiteralPath $statePath) { Get-Content -Raw -LiteralPath $statePath | ConvertFrom-Json } else { $null }
if ($state -and $state.machineTrustCreated) {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'This install created machine certificate trust. Run uninstall.ps1 as administrator to remove that certificate too.'
    }
}
Get-AppxPackage -Name 'RightClickUtilities' | ForEach-Object { Remove-AppxPackage -Package $_.PackageFullName }
if (Test-Path -LiteralPath $statePath) {
    $state = Get-Content -Raw -LiteralPath $statePath | ConvertFrom-Json
    if ($state.certificateThumbprint -match '^[0-9A-Fa-f]{40}$') {
        if ($state.trustCreated) { Remove-Item -LiteralPath "Cert:\CurrentUser\TrustedPeople\$($state.certificateThumbprint)" -ErrorAction SilentlyContinue }
        if ($state.certificateCreated) { Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($state.certificateThumbprint)" -ErrorAction SilentlyContinue }
        if ($state.machineTrustCreated) { Remove-Item -LiteralPath "Cert:\LocalMachine\TrustedPeople\$($state.certificateThumbprint)" -ErrorAction SilentlyContinue }
    }
    Remove-Item -LiteralPath $statePath
}
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class UtilitiesShellRemoveNotify { [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint e, uint f, IntPtr a, IntPtr b); }'
[UtilitiesShellRemoveNotify]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
Write-Output 'Removed the RightClick Utilities menu registration and installer-created signing certificates. Functions and application files are preserved.'
