# Explorer integration

`UtilitiesShell.dll` is an x64 native `IExplorerCommand` COM server. The sparse
identity package registers it for files (`*`) and selected folders (`Directory`)
in the Windows 11 primary context menu. Its submenu title is **RightClick Utilities**.

The DLL reads `%USERPROFILE%\.rightclick-utilities\functions.json`
each time Explorer enumerates the submenu. File/folder and extension filtering
happens in each child's `GetState`, since `EnumSubCommands` receives no selection.
Multiple selections are disabled and rejected again during invocation. Invalid
configuration yields a menu containing only **Manage functions…**. Missing
configuration uses the four built-in video functions.

Explorer starts `Utilities.exe` beside the DLL using `CreateProcessW` and Windows
argument quoting. A function receives `--run-function <id> --input <path>`;
**Manage functions…** receives `--input <path>` when a selection is available.
The shell DLL performs no media processing and never executes configuration's
`executable` or `args` itself.

## Build and verify

From the repository root:

```powershell
.\scripts\build-shell.ps1 -Test
```

Requires Visual Studio 2022 Community C++ tools and Windows SDK 10.0.26100.0.
The DLL links the C++ runtime statically. The script generates package/logo
assets and an unsigned `release\Utilities.identity.msix` alongside the DLL.
The parent app build supplies `Utilities.exe`, `UtilitiesCli.exe`, and FFmpeg.

The native harness uses actual shell selections and the built DLL. It checks
COM activation/enumeration/lifetime, dynamic configuration reads, filtering,
multiple-selection rejection, Unicode quoting, and actual process-launch argv
against a test executable. It does not change the user's functions or launch
the production GUI.

## Per-user installation

```powershell
.\scripts\install.ps1
.\scripts\uninstall.ps1
```

The installer copies the release to
`%USERPROFILE%\.rightclick-utilities\app`, creates a non-exportable
local code-signing certificate in `CurrentUser\My`, imports its public certificate
into `CurrentUser\TrustedPeople`, signs the identity MSIX, and registers it with
`Add-AppxPackage -ExternalLocation`. State records certificate ownership, including
on deployment failure. Uninstall removes registration and only those certificates
created by this installer, preserving functions and application files.

App files and shared configuration live outside AppData because a packaged host
can redirect AppData writes to its private package cache. Explorer and the package
deployment service must see the same physical files as the GUI and CLI.

Windows deployment policy and certificate trust can reject local signing. The
installer includes Windows' error and ActivityId; use `Get-AppPackageLog` for its
cause. On the tested Windows build 26200, the local certificate needed one
elevated trust setup; `CurrentUser\TrustedPeople` alone returned `0x800B0109`.
After running the installer, open an elevated PowerShell as the same user:

```powershell
.\package\install-with-machine-trust.ps1
```

The trust helper explicitly imports only the recorded Utilities public
certificate into `LocalMachine\TrustedPeople`, signs the package, and registers
it for the current user. This signed sparse package was successfully registered
with Windows reporting **Status: Ok**. Subsequent package registration is per-user
and can reuse that certificate trust. Installer state tracks the trust, so run
`uninstall.ps1` elevated to remove the installer-created machine certificate.
The helper validates both certificate thumbprint and subject.

`-Development` instead uses loose-manifest registration and requires Windows
Developer Mode. No script enables Developer Mode or restarts Explorer. If
registration remains cached, sign out and back in.

## Microsoft references

- [Modern File Explorer context menu registration](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/integrate-packaged-app-with-file-explorer)
- [Sparse package identity and per-user registration](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps)
- [Add-AppxPackage](https://learn.microsoft.com/en-us/powershell/module/appx/add-appxpackage)
