# Current-user cleanup only. This build has no service and never writes BIOS.
# Does not delete any preferences automatically; prints the exact optional path.
$config = Join-Path $env:APPDATA 'org.chargekeeper.desktop'
$local = Join-Path $env:LOCALAPPDATA 'org.chargekeeper.desktop'
Write-Output 'First turn off Launch at login in Chargekeeper, then quit using the tray menu.'
Write-Output 'Uninstall the development package with Windows Installed apps.'
Write-Output "Optional preference cleanup: $config"
Write-Output "Optional diagnostics cleanup: $local"
Write-Output 'No BIOS restoration is necessary for this read-only development build.'
