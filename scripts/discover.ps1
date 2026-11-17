# Read-only. Returns deliberately selected fields, never service tag or serial.
# wmi_status: not_checked | present | absent | denied | error (access denial is never reported as absence).
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch { }
$result = [ordered]@{ manufacturer = ''; model = ''; bios = ''; wmi = $false; cctk = $false; wmi_status = 'not_checked'; note = '' }
function Get-FailureKind($failure) {
    $e = $failure.Exception
    $code = ''
    if ($e.PSObject.Properties['NativeErrorCode']) { $code = [string]$e.NativeErrorCode }
    $hr = $e.HResult
    if ($e -is [System.UnauthorizedAccessException] -or $code -eq 'AccessDenied' -or $hr -eq -2147217405 -or $hr -eq -2147024891) { return 'denied' }
    if ($code -in @('InvalidNamespace', 'InvalidClass', 'NotFound') -or $hr -eq -2147217394 -or $hr -eq -2147217392) { return 'absent' }
    return 'error'
}
try {
    $computer = Get-CimInstance -ClassName Win32_ComputerSystem
    $firmware = Get-CimInstance -ClassName Win32_BIOS
    $result.manufacturer = [string]$computer.Manufacturer
    $result.model = [string]$computer.Model
    $result.bios = [string]$firmware.SMBIOSBIOSVersion
    if ($result.manufacturer -match '^Dell') {
        try {
            $null = Get-CimClass -Namespace root\dcim\sysman -ClassName DCIM_BIOSEnumeration
            $service = Get-CimClass -Namespace root\dcim\sysman -ClassName DCIM_BIOSService
            $result.wmi = $null -ne $service.CimClassMethods['SetBIOSAttributes']
            $result.wmi_status = if ($result.wmi) { 'present' } else { 'absent' }
        } catch { $result.wmi_status = Get-FailureKind $_ }
        $paths = @(
            'C:\Program Files (x86)\Dell\Command Configure\X86_64\cctk.exe',
            'C:\Program Files\Dell\Command Configure\X86_64\cctk.exe'
        )
        $result.cctk = @($paths | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf }).Count -gt 0
        if ($result.wmi_status -eq 'denied') {
            $result.note = 'Access to Dell Command | Monitor WMI was denied. Charging controls remain unavailable.'
        } elseif ($result.wmi_status -eq 'error') {
            $result.note = 'Dell Command | Monitor WMI could not be queried. Charging controls remain unavailable.'
        } elseif (-not $result.wmi -and -not $result.cctk) {
            $result.note = 'Install Dell Command | Monitor or Dell Command | Configure. Charging controls remain unavailable.'
        } else {
            $result.note = 'Dell interface detected; charge attributes, bounds, credentials and write support are not yet validated.'
        }
    } else { $result.note = 'No validated hardware controller for this device. Software reminders only.' }
} catch {
    if ((Get-FailureKind $_) -eq 'denied') {
        $result.note = 'Windows hardware discovery was denied access. Software reminders only.'
    } else {
        $result.note = 'Windows hardware discovery unavailable. Software reminders only.'
    }
}
$result | ConvertTo-Json -Compress
