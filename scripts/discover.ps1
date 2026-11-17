# Read-only. Returns deliberately selected fields, never service tag or serial.
# No WMI method is invoked; only class definitions and instance queries are read.
# wmi_status: not_checked | present | absent | denied | error (access denial is never reported as absence).
#   Refers to the Dell Command | Monitor classes (DCIM_*) only, unchanged for compatibility.
# dell_bios_provider: true when the built-in Dell BIOS WMI provider's class definitions
#   (root\dcim\sysman\biosattributes EnumerationAttribute, IntegerAttribute,
#   BIOSAttributeInterface) exist. Definitions are readable without elevation.
# dell_bios_read_status: not_checked | present | absent | denied | error for reading
#   attribute instances. A normal user is normally denied.
# cycle_count: null when the firmware does not report it (it reports 0).
# temperature_available: false unless a battery temperature reading exists.
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch { }
$result = [ordered]@{
    manufacturer = ''; model = ''; bios = ''; wmi = $false; cctk = $false; wmi_status = 'not_checked'
    dell_bios_provider = $false; dell_bios_read_status = 'not_checked'
    cycle_count = $null; temperature_available = $false; note = ''
}
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
        # Built-in platform BIOS provider: class definitions only (no instances, no methods).
        $providerStatus = 'absent'
        try {
            $ns = 'root\dcim\sysman\biosattributes'
            $null = Get-CimClass -Namespace $ns -ClassName EnumerationAttribute
            $null = Get-CimClass -Namespace $ns -ClassName IntegerAttribute
            $null = Get-CimClass -Namespace $ns -ClassName BIOSAttributeInterface
            $result.dell_bios_provider = $true
            $providerStatus = 'present'
        } catch { $providerStatus = Get-FailureKind $_ }
        if ($result.dell_bios_provider) {
            # Instance query only; the result is discarded, only success/failure is reported.
            try {
                $null = Get-CimInstance -Namespace root\dcim\sysman\biosattributes -ClassName EnumerationAttribute |
                    Select-Object -First 1
                $result.dell_bios_read_status = 'present'
            } catch { $result.dell_bios_read_status = Get-FailureKind $_ }
        } else {
            $result.dell_bios_read_status = $providerStatus
        }
        $paths = @(
            'C:\Program Files (x86)\Dell\Command Configure\X86_64\cctk.exe',
            'C:\Program Files\Dell\Command Configure\X86_64\cctk.exe'
        )
        $result.cctk = @($paths | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf }).Count -gt 0
        if ($result.dell_bios_provider -and -not $result.wmi -and -not $result.cctk) {
            if ($result.dell_bios_read_status -eq 'present') {
                $result.note = 'Dell built-in BIOS WMI provider detected and readable; charge attributes, bounds, credentials and write support are not yet validated. Charging controls remain unavailable.'
            } elseif ($result.dell_bios_read_status -eq 'denied') {
                $result.note = 'Dell built-in BIOS WMI provider detected; reading its settings needs administrator rights. Charging controls remain unavailable.'
            } else {
                $result.note = 'Dell built-in BIOS WMI provider detected, but its settings could not be read. Charging controls remain unavailable.'
            }
        } elseif ($result.wmi_status -eq 'denied') {
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
# Battery facts (any manufacturer). Firmware that does not report a cycle count gives 0.
try {
    $cycles = @(Get-CimInstance -Namespace root\wmi -ClassName BatteryCycleCount |
        ForEach-Object { [int64]$_.CycleCount } | Where-Object { $_ -gt 0 -and $_ -le 100000 })
    if ($cycles.Count -gt 0) { $result.cycle_count = [int](($cycles | Measure-Object -Maximum).Maximum) }
} catch { }
try {
    # Temperature is in tenths of a kelvin; 0 or missing means no reading.
    $temps = @(Get-CimInstance -Namespace root\wmi -ClassName BatteryTemperature |
        Where-Object { [int64]$_.Temperature -gt 0 })
    $result.temperature_available = $temps.Count -gt 0
} catch { }
$result | ConvertTo-Json -Compress
