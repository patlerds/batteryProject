# Read-only. Returns deliberately selected fields, never service tag or serial.
$ErrorActionPreference = 'Stop'
$result = [ordered]@{ manufacturer = ''; model = ''; bios = ''; wmi = $false; cctk = $false; note = '' }
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
        } catch { $result.note = 'Dell Command Monitor WMI is not available or access was denied.' }
        $paths = @(
            'C:\Program Files (x86)\Dell\Command Configure\X86_64\cctk.exe',
            'C:\Program Files\Dell\Command Configure\X86_64\cctk.exe'
        )
        $result.cctk = @($paths | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf }).Count -gt 0
        if (-not $result.wmi -and -not $result.cctk) {
            $result.note = 'Install Dell Command | Monitor or Dell Command | Configure. Charging controls remain unavailable.'
        } else {
            $result.note = 'Dell interface detected; charge attributes, bounds, credentials and write support are not yet validated.'
        }
    } else { $result.note = 'No validated hardware controller for this device. Software reminders only.' }
} catch { $result.note = 'Windows hardware discovery unavailable or access denied. Software reminders only.' }
$result | ConvertTo-Json -Compress
