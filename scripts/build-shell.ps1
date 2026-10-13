[CmdletBinding()]
param([string]$OutputDirectory = (Join-Path $PSScriptRoot '..\release'), [switch]$Test)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$output = [IO.Path]::GetFullPath($OutputDirectory)
$build = Join-Path $root 'shell\build'
$vs = 'C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat'
$sdk = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64'
foreach ($tool in @($vs, (Join-Path $sdk 'MakeAppx.exe'))) {
    if (!(Test-Path -LiteralPath $tool)) { throw "Required build tool missing: $tool" }
}
New-Item -ItemType Directory -Force -Path $output, $build, (Join-Path $output 'Assets') | Out-Null
$commands = @(
    "@call `"$vs`" -arch=x64 -host_arch=x64 >nul",
    "@cl /nologo /std:c++17 /EHsc /O2 /W4 /MT /DUNICODE /D_UNICODE /LD `"$root\shell\UtilitiesShell.cpp`" /Fo`"$build\UtilitiesShell.obj`" /Fe`"$output\UtilitiesShell.dll`" /link /DEF:`"$root\shell\UtilitiesShell.def`" /IMPLIB:`"$build\UtilitiesShell.lib`" shell32.lib shlwapi.lib ole32.lib windowsapp.lib",
    '@if errorlevel 1 exit /b 1'
)
if ($Test) {
    $commands += "@cl /nologo /std:c++17 /EHsc /O2 /W4 /MT /DUNICODE /D_UNICODE `"$root\shell\tests\test_shell.cpp`" /Fo`"$build\test_shell.obj`" /Fe`"$build\test_shell.exe`" /link shell32.lib shlwapi.lib ole32.lib windowsapp.lib"
    $commands += '@if errorlevel 1 exit /b 1'
    $commands += "@cl /nologo /std:c++17 /EHsc /O2 /W4 /MT /DUNICODE /D_UNICODE `"$root\shell\tests\launch_probe.cpp`" /Fo`"$build\launch_probe.obj`" /Fe`"$build\launch_probe.exe`" /link /SUBSYSTEM:WINDOWS shell32.lib"
    $commands += '@if errorlevel 1 exit /b 1'
}
$batch = Join-Path $build 'compile.cmd'
[IO.File]::WriteAllLines($batch, $commands, [Text.Encoding]::Default)
& $env:ComSpec /d /c "`"$batch`""
if ($LASTEXITCODE) { throw "Native shell build failed ($LASTEXITCODE)." }

# Draw the package's simple four-tile utility mark without external image assets.
Add-Type -AssemblyName System.Drawing
foreach ($asset in @(@('StoreLogo.png',50), @('Square44x44Logo.png',44), @('Square150x150Logo.png',150))) {
    $size = [int]$asset[1]
    $bitmap = New-Object Drawing.Bitmap $size, $size
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.Clear([Drawing.Color]::Transparent)
    $brush = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(255, 45, 111, 218))
    $pad = [int]($size * 0.14)
    $tile = [int]($size * 0.30)
    $gap = [int]($size * 0.10)
    foreach ($x in @($pad, ($pad + $tile + $gap))) {
        foreach ($y in @($pad, ($pad + $tile + $gap))) { $graphics.FillRectangle($brush, $x, $y, $tile, $tile) }
    }
    $bitmap.Save((Join-Path $output ('Assets\' + $asset[0])), [Drawing.Imaging.ImageFormat]::Png)
    if ($size -eq 44) {
        # PNG-in-ICO, supported by Windows Explorer.
        $stream = New-Object IO.MemoryStream
        $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
        $bytes = $stream.ToArray()
        $iconStream = [IO.File]::Create((Join-Path $output 'Assets\Utilities.ico'))
        $writer = New-Object IO.BinaryWriter $iconStream
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]1)
        $writer.Write([byte]44); $writer.Write([byte]44); $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([uint32]$bytes.Length); $writer.Write([uint32]22)
        $writer.Write($bytes); $writer.Dispose(); $stream.Dispose()
    }
    $brush.Dispose(); $graphics.Dispose(); $bitmap.Dispose()
}
$identity = Join-Path $build 'identity'
New-Item -ItemType Directory -Force -Path $identity | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'package\AppxManifest.xml') -Destination (Join-Path $identity 'AppxManifest.xml')
Copy-Item -LiteralPath (Join-Path $root 'package\AppxManifest.xml') -Destination (Join-Path $output 'AppxManifest.xml')
# Sparse packages resolve binaries and assets at the registered external location.
& (Join-Path $sdk 'MakeAppx.exe') pack /o /d $identity /nv /p (Join-Path $output 'Utilities.identity.msix')
if ($LASTEXITCODE) { throw "Sparse package build failed ($LASTEXITCODE)." }
if ($Test) {
    & (Join-Path $build 'test_shell.exe') (Join-Path $output 'UtilitiesShell.dll')
    if ($LASTEXITCODE) { throw "Shell tests failed ($LASTEXITCODE)." }
}
Write-Output "Built shell DLL and unsigned identity package in $output"
