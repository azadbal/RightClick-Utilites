[CmdletBinding()]
param(
    [string]$SourceDirectory = (Join-Path $PSScriptRoot '..\release'),
    [string]$InstallDirectory = (Join-Path $env:USERPROFILE '.azad-utilities\right-click-menu\app'),
    [switch]$Development
)
$ErrorActionPreference = 'Stop'
$source = [IO.Path]::GetFullPath($SourceDirectory)
$target = [IO.Path]::GetFullPath($InstallDirectory)
$name = 'AzadUtilities.RightClickMenu'
$subject = 'CN=Azad Utilities Local Development'
foreach ($file in @('Utilities.exe','UtilitiesCli.exe','UtilitiesShell.dll','AppxManifest.xml','Utilities.identity.msix')) {
    if (!(Test-Path -LiteralPath (Join-Path $source $file))) { throw "Build the complete release first. Missing $file in $source" }
}
if ([Environment]::OSVersion.Version.Build -lt 22000) { throw 'The modern context menu requires Windows 11 (build 22000 or later).' }
$configDirectory = Join-Path $env:USERPROFILE '.azad-utilities\right-click-menu'
New-Item -ItemType Directory -Force -Path $target, $configDirectory | Out-Null
$statePath = Join-Path $configDirectory 'install-state.json'
$state = if (Test-Path -LiteralPath $statePath) { Get-Content -Raw -LiteralPath $statePath | ConvertFrom-Json } else { $null }
# Older installs launched by a packaged host may have redirected AppData writes.
# Reading their state preserves certificate ownership without touching function settings.
$legacyStatePath = Join-Path $env:LOCALAPPDATA 'AzadUtilities\RightClickMenu\install-state.json'
if (!$state -and (Test-Path -LiteralPath $legacyStatePath)) {
    $state = Get-Content -Raw -LiteralPath $legacyStatePath | ConvertFrom-Json
}
$oldPackages = @(Get-AppxPackage -Name $name)
# Stop neither Explorer nor the utility. A loaded DLL may require sign-out before updating.
if ($source -ne $target) {
    Get-ChildItem -LiteralPath $source | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $target -Recurse -Force }
}
$certificateThumbprint = if ($state) { $state.certificateThumbprint } else { $null }
$certificateCreated = if ($state) { [bool]$state.certificateCreated } else { $false }
$trustCreated = if ($state) { [bool]$state.trustCreated } else { $false }
$machineTrustCreated = if ($state -and $state.PSObject.Properties['machineTrustCreated']) { [bool]$state.machineTrustCreated } else { $false }
function Save-InstallState {
    @{installDirectory=$target;certificateThumbprint=$certificateThumbprint;certificateCreated=$certificateCreated;trustCreated=$trustCreated;machineTrustCreated=$machineTrustCreated;development=[bool]$Development;unsigned=$false;userSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value} | ConvertTo-Json | Set-Content -LiteralPath $statePath -Encoding UTF8
    if (Test-Path -LiteralPath $legacyStatePath) { Remove-Item -LiteralPath $legacyStatePath }
}
if (!$Development) {
    $signTool = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\SignTool.exe'
    if (!(Test-Path -LiteralPath $signTool)) { throw 'Windows SDK SignTool is required for local signing. Use -Development only when Windows Developer Mode is enabled.' }
    $certificate = if ($certificateThumbprint) { Get-Item -LiteralPath "Cert:\CurrentUser\My\$certificateThumbprint" -ErrorAction SilentlyContinue } else { $null }
    if (!$certificate) {
        $certificate = New-SelfSignedCertificate -Type Custom -Subject $subject -KeyUsage DigitalSignature -KeyExportPolicy NonExportable -FriendlyName 'Utilities local MSIX signing' -CertStoreLocation 'Cert:\CurrentUser\My' -NotAfter (Get-Date).AddYears(2) -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3','2.5.29.19={text}')
        $certificateCreated = $true
        $certificateThumbprint = $certificate.Thumbprint
        Save-InstallState
    }
    $publicCert = Join-Path $target 'Utilities-local.cer'
    Export-Certificate -Cert $certificate -FilePath $publicCert -Force | Out-Null
    if (!(Test-Path -LiteralPath "Cert:\CurrentUser\TrustedPeople\$certificateThumbprint")) {
        Import-Certificate -FilePath $publicCert -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
        $trustCreated = $true
        Save-InstallState
    }
    & $signTool sign /fd SHA256 /s My /sha1 $certificateThumbprint (Join-Path $target 'Utilities.identity.msix')
    if ($LASTEXITCODE) { throw "Signing failed ($LASTEXITCODE)." }
}
# Persist owned certificate metadata even if Windows policy blocks package deployment.
Save-InstallState
try {
    foreach ($package in $oldPackages) { Remove-AppxPackage -Package $package.PackageFullName }
    if ($Development) { Add-AppxPackage -Path (Join-Path $target 'AppxManifest.xml') -Register -ExternalLocation $target }
    else { Add-AppxPackage -Path (Join-Path $target 'Utilities.identity.msix') -ExternalLocation $target }
} catch {
    throw "Windows rejected package registration: $($_.Exception.Message) Run Get-AppPackageLog with the ActivityId above for the deployment cause. Run uninstall.ps1 to remove this registration and installer-created certificates."
}
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class UtilitiesShellNotify { [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint e, uint f, IntPtr a, IntPtr b); }'
[UtilitiesShellNotify]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
Get-AppxPackage -Name $name | Select-Object Name,PackageFullName,Status
Write-Output "Installed RightClick Utilities for the current user at $target. If Explorer has cached its old menu, sign out and back in."
