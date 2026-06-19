param(
    [switch]$Launch
)

$ErrorActionPreference = "Stop"

$AppName = "Windows App Delete Controller"
$AppId = "WinAppDeleteController"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\$AppId"
$ExeName = "win_app_delete_controller.exe"
$SourceExe = Join-Path $PSScriptRoot $ExeName
$FallbackExe = Join-Path (Split-Path $PSScriptRoot -Parent) "target\release\$ExeName"

if (Test-Path $FallbackExe) {
    $SourceExe = $FallbackExe
} elseif (-not (Test-Path $SourceExe)) {
    throw "Cannot find $ExeName. Run cargo build --release first."
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$InstalledExe = Join-Path $InstallDir $ExeName
Copy-Item -Force -Path $SourceExe -Destination $InstalledExe

$UninstallScript = Join-Path $InstallDir "uninstall.ps1"
@"
`$ErrorActionPreference = "Stop"
`$AppName = "$AppName"
`$AppId = "$AppId"
`$InstallDir = Join-Path `$env:LOCALAPPDATA "Programs\`$AppId"
`$DesktopShortcut = Join-Path ([Environment]::GetFolderPath("Desktop")) "`$AppName.lnk"
`$StartMenuDir = Join-Path ([Environment]::GetFolderPath("Programs")) "`$AppName"
if (Test-Path `$DesktopShortcut) { Remove-Item -Force `$DesktopShortcut }
if (Test-Path `$StartMenuDir) { Remove-Item -Recurse -Force `$StartMenuDir }
Start-Sleep -Milliseconds 300
if (Test-Path `$InstallDir) { Remove-Item -Recurse -Force `$InstallDir }
"@ | Set-Content -Encoding UTF8 -Path $UninstallScript

function New-AppShortcut {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Target,
        [string]$Arguments = "",
        [string]$WorkingDirectory = ""
    )

    $Shell = New-Object -ComObject WScript.Shell
    $Shortcut = $Shell.CreateShortcut($Path)
    $Shortcut.TargetPath = $Target
    $Shortcut.Arguments = $Arguments
    $Shortcut.WorkingDirectory = $WorkingDirectory
    $Shortcut.IconLocation = $Target
    $Shortcut.Save()
}

$DesktopShortcut = Join-Path ([Environment]::GetFolderPath("Desktop")) "$AppName.lnk"
New-AppShortcut -Path $DesktopShortcut -Target $InstalledExe -WorkingDirectory $InstallDir

$StartMenuDir = Join-Path ([Environment]::GetFolderPath("Programs")) $AppName
New-Item -ItemType Directory -Force -Path $StartMenuDir | Out-Null
New-AppShortcut `
    -Path (Join-Path $StartMenuDir "$AppName.lnk") `
    -Target $InstalledExe `
    -WorkingDirectory $InstallDir
New-AppShortcut `
    -Path (Join-Path $StartMenuDir "Uninstall $AppName.lnk") `
    -Target "powershell.exe" `
    -Arguments "-NoProfile -ExecutionPolicy Bypass -File `"$UninstallScript`"" `
    -WorkingDirectory $InstallDir

Write-Host "$AppName installed to $InstallDir"
Write-Host "Desktop and Start Menu shortcuts were created."

if ($Launch) {
    Start-Process -FilePath $InstalledExe -WorkingDirectory $InstallDir
}
