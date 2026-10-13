[CmdletBinding()]
param([string]$StatePath = (Join-Path $env:USERPROFILE '.rightclick-utilities\install-state.json'))
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script as administrator. It trusts only the existing Utilities signing certificate and registers the package for the current user.'
}
$state = Get-Content -Raw -LiteralPath $StatePath | ConvertFrom-Json
Start-Transcript -Path (Join-Path (Split-Path -Parent $StatePath) 'install-admin.log') -Force | Out-Null
if ($state.userSid -and $state.userSid -ne $identity.User.Value) { throw 'Elevate as the same user who ran install.ps1; registration is per-user.' }
if ($state.certificateThumbprint -notmatch '^[0-9A-Fa-f]{40}$') { throw 'Installer signing certificate metadata is missing.' }
$certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $state.installDirectory 'Utilities-local.cer'))
if ($certificate.Thumbprint -ne $state.certificateThumbprint -or $certificate.Subject -ne 'CN=RightClick Utilities Local Development') { throw 'Public certificate does not match the installer state.' }
$signTool = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\SignTool.exe'
& $signTool sign /fd SHA256 /s My /sha1 $certificate.Thumbprint (Join-Path $state.installDirectory 'Utilities.identity.msix')
if ($LASTEXITCODE) { throw "Package signing failed ($LASTEXITCODE)." }
if (!(Test-Path -LiteralPath "Cert:\LocalMachine\TrustedPeople\$($certificate.Thumbprint)")) {
    Import-Certificate -FilePath (Join-Path $state.installDirectory 'Utilities-local.cer') -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null
    $state | Add-Member -NotePropertyName machineTrustCreated -NotePropertyValue $true -Force
    $state | ConvertTo-Json | Set-Content -LiteralPath $StatePath -Encoding UTF8
}
Get-AppxPackage -Name 'RightClickUtilities' | ForEach-Object { Remove-AppxPackage -Package $_.PackageFullName }
Add-AppxPackage -Path (Join-Path $state.installDirectory 'Utilities.identity.msix') -ExternalLocation $state.installDirectory
$state.unsigned = $false
$state.development = $false
$state | ConvertTo-Json | Set-Content -LiteralPath $StatePath -Encoding UTF8
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class UtilitiesAdminNotify { [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint e, uint f, IntPtr a, IntPtr b); }'
[UtilitiesAdminNotify]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
Get-AppxPackage -Name 'RightClickUtilities' | Select-Object Name,PackageFullName,Status
Stop-Transcript | Out-Null
