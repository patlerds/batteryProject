#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Chargekeeper: one-off, STRICTLY READ-ONLY read of Dell built-in BIOS battery/charge settings.

.DESCRIPTION
    Run this yourself from an administrator PowerShell. It changes nothing.

    What it reads (instance queries only, via Get-CimInstance):
      - root\dcim\sysman\biosattributes  EnumerationAttribute and IntegerAttribute,
        keeping only attributes whose name contains "battery", "batt" or "charge"
        (case-insensitive).
      - root\dcim\sysman\wmisecurity     PasswordObject, keeping only NameId and
        IsPasswordSet (whether a BIOS password is set, never the password).

    What it does NOT do: it calls no WMI method (no Invoke-CimMethod, no
    SetAttribute), runs no Set-* cmdlet, writes no registry value, uses no network,
    and runs no Dell tool. It does not read the service tag, serial numbers or GUIDs.

    Output: a JSON file (default .\dell-battery-read.json in the current folder) with,
    per attribute: name, current value, possible values, lower/upper bound and
    increment; and per password NameId: password set yes/no. A short summary is printed.

.PARAMETER OutFile
    Where to write the JSON. Default: .\dell-battery-read.json

.PARAMETER SelfTest
    Validates the filtering and output shaping with fake objects. Queries no WMI.
    #Requires applies to the whole file, so a developer without admin can run the
    self-test by loading the text without its first line, for example:
      & ([scriptblock]::Create(((Get-Content -Raw .\scripts\dell-read-elevated.ps1) -replace '^#Requires[^\r\n]*', ''))) -SelfTest

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\dell-read-elevated.ps1
#>
[CmdletBinding()]
param(
    [string]$OutFile = '.\dell-battery-read.json',
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'

$NamePattern = '(?i)batt|charge'

function Select-ChargeAttribute {
    param([object[]]$Items, [ValidateSet('Enumeration', 'Integer')][string]$Kind)
    foreach ($item in @($Items)) {
        if ($null -eq $item) { continue }
        $name = [string]$item.AttributeName
        if ($name -notmatch $NamePattern) { continue }
        $entry = [ordered]@{ name = $name; current_value = $null }
        if ($Kind -eq 'Enumeration') {
            $entry.current_value = @($item.CurrentValue | ForEach-Object { [string]$_ })
            $entry.possible_values = @($item.PossibleValue | ForEach-Object { [string]$_ })
        } else {
            $entry.current_value = @($item.CurrentValue | ForEach-Object { [int64]$_ })
            $entry.lower_bound = if ($null -ne $item.LowerBound) { [int64]$item.LowerBound } else { $null }
            $entry.upper_bound = if ($null -ne $item.UpperBound) { [int64]$item.UpperBound } else { $null }
            $entry.increment = if ($null -ne $item.ScalarIncrement) { [int64]$item.ScalarIncrement } else { $null }
        }
        [pscustomobject]$entry
    }
}

function Select-PasswordState {
    param([object[]]$Items)
    foreach ($item in @($Items)) {
        if ($null -eq $item) { continue }
        $set = $item.IsPasswordSet
        $state = if ($null -eq $set) { 'unknown' } elseif ([int64]$set -ne 0) { 'yes' } else { 'no' }
        [pscustomobject][ordered]@{ name_id = [string]$item.NameId; password_set = $state }
    }
}

function New-Report {
    param([object[]]$Enumerations, [object[]]$Integers, [object[]]$Passwords, [hashtable]$Status)
    $attributes = @(Select-ChargeAttribute -Items $Enumerations -Kind Enumeration) +
        @(Select-ChargeAttribute -Items $Integers -Kind Integer)
    [ordered]@{
        tool = 'chargekeeper dell-read-elevated (read-only)'
        read_at = (Get-Date).ToString('yyyy-MM-ddTHH:mm:ssK')
        status = $Status
        attributes = @($attributes | Sort-Object name)
        passwords = @(Select-PasswordState -Items $Passwords)
    }
}

function Get-FailureKind($failure) {
    $e = $failure.Exception
    $code = ''
    if ($e.PSObject.Properties['NativeErrorCode']) { $code = [string]$e.NativeErrorCode }
    if ($e -is [System.UnauthorizedAccessException] -or $code -eq 'AccessDenied') { return 'denied' }
    if ($code -in @('InvalidNamespace', 'InvalidClass', 'NotFound')) { return 'absent' }
    return 'error'
}

function Write-Summary($report, $path) {
    Write-Host "Read-only Dell BIOS read complete. Nothing was changed."
    Write-Host ("Status: enumeration={0} integer={1} password={2}" -f $report.status.enumeration, $report.status.integer, $report.status.password)
    Write-Host ("Battery/charge attributes found: {0}" -f @($report.attributes).Count)
    foreach ($a in $report.attributes) {
        $line = "  {0} = {1}" -f $a.name, (@($a.current_value) -join ',')
        if ($a.PSObject.Properties['lower_bound']) { $line += " (range {0}..{1}, step {2})" -f $a.lower_bound, $a.upper_bound, $a.increment }
        elseif (@($a.possible_values).Count) { $line += " (options: {0})" -f (@($a.possible_values) -join ', ') }
        Write-Host $line
    }
    foreach ($p in $report.passwords) { Write-Host ("  Password {0}: set={1}" -f $p.name_id, $p.password_set) }
    if ($path) { Write-Host "Saved to $path" }
}

if ($SelfTest) {
    $failures = 0
    function Assert($cond, $msg) { if (-not $cond) { Write-Host "FAIL: $msg"; $script:failures++ } else { Write-Host "ok:   $msg" } }
    $enum = @(
        [pscustomobject]@{ AttributeName = 'PrimaryBattChargeCfg'; CurrentValue = @('Adaptive'); PossibleValue = @('Standard', 'Adaptive', 'Custom'); InstanceName = 'SECRET-GUID-1'; DisplayName = 'x' },
        [pscustomobject]@{ AttributeName = 'BootMode'; CurrentValue = @('UEFI'); PossibleValue = @('UEFI') },
        [pscustomobject]@{ AttributeName = 'usbpowershare'; CurrentValue = @('Enabled'); PossibleValue = @('Enabled') },
        [pscustomobject]@{ AttributeName = 'ADVBATTCHGMODE'; CurrentValue = @('Disabled'); PossibleValue = @('Disabled', 'Enabled') },
        $null
    )
    $int = @(
        [pscustomobject]@{ AttributeName = 'CustomChargeStart'; CurrentValue = @(50); LowerBound = 50; UpperBound = 95; ScalarIncrement = 1; InstanceName = 'SECRET-GUID-2' },
        [pscustomobject]@{ AttributeName = 'customchargestop'; CurrentValue = @(90); LowerBound = 55; UpperBound = 100; ScalarIncrement = 1 },
        [pscustomobject]@{ AttributeName = 'AutoOnHour'; CurrentValue = @(0); LowerBound = 0; UpperBound = 23; ScalarIncrement = 1 }
    )
    $pw = @(
        [pscustomobject]@{ NameId = 'Admin'; IsPasswordSet = 1; InstanceName = 'SECRET-GUID-3'; MaximumPasswordLength = 32 },
        [pscustomobject]@{ NameId = 'System'; IsPasswordSet = 0 },
        [pscustomobject]@{ NameId = 'HDD'; IsPasswordSet = $null }
    )
    $report = New-Report -Enumerations $enum -Integers $int -Passwords $pw -Status @{ enumeration = 'present'; integer = 'present'; password = 'present' }
    $names = @($report.attributes | ForEach-Object { $_.name })
    Assert ($names.Count -eq 4) "keeps 4 battery/charge attributes (got $($names -join ','))"
    Assert (-not ($names -contains 'BootMode') -and -not ($names -contains 'AutoOnHour') -and -not ($names -contains 'usbpowershare')) 'drops unrelated attributes'
    Assert (($names -contains 'customchargestop') -and ($names -contains 'ADVBATTCHGMODE')) 'name match is case-insensitive'
    $start = $report.attributes | Where-Object name -eq 'CustomChargeStart'
    Assert ($start.lower_bound -eq 50 -and $start.upper_bound -eq 95 -and $start.increment -eq 1 -and $start.current_value[0] -eq 50) 'integer bounds/increment/current shaped'
    $cfg = $report.attributes | Where-Object name -eq 'PrimaryBattChargeCfg'
    Assert (($cfg.possible_values -join ',') -eq 'Standard,Adaptive,Custom' -and $cfg.current_value[0] -eq 'Adaptive') 'enumeration current/possible shaped'
    $pwMap = @{}; $report.passwords | ForEach-Object { $pwMap[$_.name_id] = $_.password_set }
    Assert ($pwMap['Admin'] -eq 'yes' -and $pwMap['System'] -eq 'no' -and $pwMap['HDD'] -eq 'unknown') 'password set yes/no/unknown per NameId'
    $json = $report | ConvertTo-Json -Depth 6
    Assert ($json -notmatch 'SECRET|InstanceName|DisplayName|MaximumPasswordLength') 'no GUIDs/instance names or extra properties in output'
    $allowed = @('name', 'current_value', 'possible_values', 'lower_bound', 'upper_bound', 'increment')
    $extra = @($report.attributes | ForEach-Object { $_.PSObject.Properties.Name } | Where-Object { $_ -notin $allowed })
    Assert ($extra.Count -eq 0) 'attribute objects contain only allowed fields'
    $empty = New-Report -Enumerations @() -Integers @() -Passwords @() -Status @{ enumeration = 'denied'; integer = 'denied'; password = 'denied' }
    Assert (@($empty.attributes).Count -eq 0 -and @($empty.passwords).Count -eq 0) 'empty/denied input gives empty lists'
    $null = $empty | ConvertTo-Json -Depth 6
    Write-Summary $report $null
    if ($failures) { Write-Host "SELF-TEST FAILED ($failures)"; exit 1 }
    Write-Host 'SELF-TEST PASSED'
    exit 0
}

$status = @{ enumeration = 'not_checked'; integer = 'not_checked'; password = 'not_checked' }
$enumerations = @(); $integers = @(); $passwords = @()
$ns = 'root\dcim\sysman\biosattributes'
try { $enumerations = @(Get-CimInstance -Namespace $ns -ClassName EnumerationAttribute); $status.enumeration = 'present' } catch { $status.enumeration = Get-FailureKind $_ }
try { $integers = @(Get-CimInstance -Namespace $ns -ClassName IntegerAttribute); $status.integer = 'present' } catch { $status.integer = Get-FailureKind $_ }
try {
    $passwords = @(Get-CimInstance -Namespace root\dcim\sysman\wmisecurity -ClassName PasswordObject -Property NameId, IsPasswordSet)
    $status.password = 'present'
} catch { $status.password = Get-FailureKind $_ }

$report = New-Report -Enumerations $enumerations -Integers $integers -Passwords $passwords -Status $status
$full = [System.IO.Path]::GetFullPath((Join-Path (Get-Location).ProviderPath $OutFile))
[System.IO.File]::WriteAllText($full, ($report | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
Write-Summary $report $full
