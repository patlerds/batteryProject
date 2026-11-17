# Device evidence and qualification

| Device | Evidence | Status |
|---|---|---|
| Dell Latitude 7320 Detachable, BIOS 1.50.0 | M-C read-only discovery (2026-11-17, below): Dell built-in BIOS WMI provider **present** (`root\dcim\sysman\biosattributes`, `wmisecurity`); attribute/password instances **access denied** to a normal user; no Dell Command software installed; battery readable via `root\wmi` and `Win32_Battery` | Identified only; charge config NOT read; charge limiting NOT verified |
| Simulated battery | Automated core integration tests | Simulated, not hardware support |
| Other Dell / Lenovo / ASUS / HP / Surface / Framework / Samsung / MSI / Acer / Huawei | Research references in PLAN | No verified write provider |

No physical laptop is currently claimed as a supported charge-limiting device.

## M-C: read-only raw discovery, Latitude 7320 Detachable

> **Everything in this section was observed read-only on 2026-11-17**, from a
> non-elevated PowerShell (Windows PowerShell 5.1, Medium integrity, Administrators
> role not active). Only class/instance *queries*, registry *reads*, `Get-PnpDevice`
> and `powercfg /batteryreport` (output to a temp folder outside the repo) were used.
> No WMI method was invoked, nothing was written to the registry or BIOS, nothing
> was installed, nothing ran elevated, and no Dell CLI tool was executed.
> Service tag, serial numbers, UUIDs/GUIDs, MACs and usernames are intentionally omitted.

### Machine summary

| Item | Value |
|---|---|
| Manufacturer / model | Dell Inc. / Latitude 7320 Detachable (System SKU `0A45`, PCSystemType 2 = mobile) |
| BIOS | 1.50.0 (SMBIOS 1.50), release date 2026-08-27 |
| Windows | Windows 11 Home 64-bit, version 26H2, `10.0.26300` UBR 9974 (`Win32_OperatingSystem` + registry). The battery report labels the OS build `26100.1.amd64fre.ge_release.240331-1435` (kernel branch label). |
| Battery | 1 pack, manufacturer BYD, chemistry LiP, design 39,224 mWh, full charge 36,488 mWh (~93.0% of design) |
| Dell software | None of Dell Command \| Configure / Monitor / Update, Dell Optimizer, Power Manager or SupportAssist is installed. Only "Dell Active Pen Service" (Wacom) is present. |

### Commands run and results

`present` = query succeeded; `denied` = WMI `AccessDenied`; `absent` = namespace/class not found; `error` = other failure.

| # | Query (read-only) | Result |
|---|---|---|
| 1 | `Get-CimInstance Win32_ComputerSystem / Win32_BIOS / Win32_OperatingSystem`, `Get-ItemProperty HKLM:\...\Windows NT\CurrentVersion` | present |
| 2 | `Get-CimInstance -Namespace root -ClassName __NAMESPACE` | present (includes `dcim`, `WMI`, `Intel_ME`, ...) |
| 3 | `__NAMESPACE` walk of `root\dcim` | present: `root\dcim\sysman` → `biosattributes`, `wmisecurity`, `iq`, `diagnostics` (no deeper children) |
| 4 | `__NAMESPACE` in `root\wmi` | present (only `ms_409`) |
| 5 | `Get-CimClass -Namespace root\dcim\sysman\*` (definitions only) | present, see class list below |
| 6 | `Get-CimClass root\dcim\sysman DCIM_BIOSEnumeration / DCIM_BIOSService` (Dell Command \| Monitor classes) | **absent** (`NotFound`) |
| 7 | `Get-CimInstance biosattributes EnumerationAttribute / IntegerAttribute / StringAttribute / CollectionAttribute` | **denied** (each) |
| 8 | Same, retry with WQL `WHERE AttributeName='PrimaryBattChargeCfg'` | **denied** |
| 9 | Same, retry with `Get-WmiObject IntegerAttribute` | **denied** |
| 10 | `Get-CimInstance biosattributes BIOSAttributeInterface` (instance list only) | **denied** |
| 11 | `Get-CimInstance wmisecurity PasswordObject` (CIM, then `Get-WmiObject`) | **denied** (both) |
| 12 | `root\wmi BatteryStatus` | present (1 instance) |
| 13 | `root\wmi BatteryStaticData` | `Get-CimInstance` 1st try: error ("Generic failure"); `Get-WmiObject` and CIM WQL retries: present |
| 14 | `root\wmi BatteryFullChargedCapacity` | present |
| 15 | `root\wmi BatteryCycleCount` | present, but value 0 (not reported; battery report shows "-") |
| 16 | `root\wmi BatteryRuntime` | present (sentinel 4294967295 = unknown, on AC) |
| 17 | `root\wmi BatteryTemperature` | class present, **0 instances** |
| 18 | `root\wmi MSAcpi_ThermalZoneTemperature` | error ("Not supported") |
| 19 | `Win32_PerfFormattedData_Counters_ThermalZoneInformation` | present, 0 instances |
| 20 | `Win32_TemperatureProbe` | present, 6 instances (CPU, DIMM, SKIN, OTHER, DIMM A, OTHER), all `CurrentReading` empty |
| 21 | `Win32_Battery` | present, 1 instance (the earlier "no instances" observation did not reproduce) |
| 22 | `powercfg /batteryreport` (temp folder output) | present |
| 23 | Uninstall keys (HKLM, HKLM WOW6432Node, HKCU), `HKLM\SYSTEM\CurrentControlSet\Services`, `C:\Program Files[ (x86)]\Dell` | no Dell Command/Optimizer/Power Manager entries, no Dell services, no Dell program folders |
| 24 | `Get-PnpDevice` | 7 × "Microsoft Windows Management Interface for ACPI" (`ACPI\VEN_PNP&DEV_0C14`), "System Firmware 1.50.0" (Dell), "Microsoft ACPI-Compliant Control Method Battery" — all OK |

### Dell BIOS WMI provider: classes found (definitions readable unprivileged)

All of these are `dynamic` classes served by the provider `Provider_PlatformBios`, i.e. the BIOS WMI provider built into the platform, not Dell Command | Monitor.

| Namespace | Class | Properties | Methods (NOT invoked) |
|---|---|---|---|
| `biosattributes` | `EnumerationAttribute` | AttributeName, CurrentValue, DefaultValue, DisplayName, DisplayNameLangCode, InstanceName, Modifiers, PossibleValue, PossibleValueCount, ReadOnly, ValueModifierCount, ValueModifiers | — |
| `biosattributes` | `IntegerAttribute` | AttributeName, CurrentValue, DefaultValue, DisplayName, DisplayNameLangCode, InstanceName, LowerBound, Modifiers, ReadOnly, ScalarIncrement, UpperBound | — |
| `biosattributes` | `StringAttribute` | AttributeName, CurrentValue, DefaultValue, DisplayName, ..., MaxLength, MinLength, ReadOnly | — |
| `biosattributes` | `CollectionAttribute` | AttributeCount, AttributeName, Attributes, DisplayName, ..., ReadOnly | — |
| `biosattributes` | `BootOrder` | BOCount, BootListType, BootOrder, InstanceName, IsActive | — |
| `biosattributes` | `SetBootOrder` | Active, InstanceName | Set |
| `biosattributes` | `BIOSAttributeInterface` | Active, InstanceName | SetDefaultValue, SetDefaultValues, SetBIOSDefaults, SetAttribute, SetAttributes, GetHelpString |
| `wmisecurity` | `PasswordObject` | Active, InstanceName, IsPasswordSet, MaximumPasswordLength, MinimumPasswordLength, NameId | — |
| `wmisecurity` | `SecurityInterface` | Active, InstanceName | SetNewPassword |
| `iq` | `BIQLOG` | Active, InstanceName | (BIOS IQ log/policy methods) |
| `diagnostics` | `LegacyDiags` | Active, InstanceName | Execute |

`root\dcim\sysman` itself defines no non-system classes of its own.

### Charge-configuration attributes

| Question | Answer |
|---|---|
| Attribute names (e.g. `PrimaryBattChargeCfg`, `CustomChargeStart`, `CustomChargeStop`) | **Unknown.** Instance enumeration is denied to a normal user, so names were not observed. They were *not* assumed. |
| Current values / possible values | Unknown (denied) |
| Integer min / max / step | Unknown (denied). The class exposes `LowerBound`, `UpperBound` and `ScalarIncrement`, so an authorised read should give real bounds. |
| BIOS password set (Admin / System) | **unknown** (`PasswordObject` denied) |

### Battery telemetry (root\wmi, Win32_Battery, battery report)

Snapshot at 2026-11-17 ~11:16 local time, on AC and charging:

| Source | Field | Value |
|---|---|---|
| `BatteryStatus` | PowerOnline / Charging / Discharging / Critical | True / True / False / False |
| `BatteryStatus` | RemainingCapacity | 8,033 mWh (~22%) |
| `BatteryStatus` | ChargeRate / DischargeRate | 19,912 mW / 0 |
| `BatteryStatus` | Voltage | 7,971 mV |
| `BatteryStaticData` | DesignedCapacity / Technology / Chemistry | 39,224 mWh / 1 (rechargeable) / `LiP` (ASCII-packed 5269836) |
| `BatteryFullChargedCapacity` | FullChargedCapacity | 36,488 mWh |
| `BatteryCycleCount` | CycleCount | 0 (firmware does not report; battery report shows "-") |
| `Win32_Battery` | EstimatedChargeRemaining / BatteryStatus / Chemistry | 22% / 2 / 2 (Unknown) |
| battery report | Design / full charge / cycle count | 39,224 mWh / 36,488 mWh / "-" |

**Temperature: not available anywhere probed.** `BatteryTemperature` has no instances,
`MSAcpi_ThermalZoneTemperature` is "Not supported", the perf thermal-zone counter has no instances, and
`Win32_TemperatureProbe` instances have no readings. The Dell BIOS attributes could not be read, so
whether they expose any thermal data is unknown.

### Implications for Chargekeeper

1. **Can raw WMI read the charge configuration unprivileged? No.** Every
   instance read in `biosattributes` and `wmisecurity` returns `AccessDenied` at
   Medium integrity. Unprivileged code can only detect the provider by its *class
   definitions*. Reading mode/thresholds/bounds/password status therefore belongs
   in the future privileged service (or a one-off elevated read-only probe).
2. **Would writing need admin? Yes, at minimum.** Even reads are denied, and the
   only write path seen is `BIOSAttributeInterface.SetAttribute(s)`, not tested.
   Whether a BIOS admin password is also required is unknown until
   `PasswordObject.IsPasswordSet` can be read elevated.
3. **Is Dell Command | Configure needed? Probably not.** The built-in platform
   provider (`Provider_PlatformBios`) exposes the attribute/bounds/password model
   on this BIOS without any Dell software. This still isn't proven for *battery*
   attributes: an elevated read-only enumeration must confirm the battery
   attributes exist before relying on it. Dell Command | Monitor is also not
   needed for detection, and its `DCIM_*` classes are absent.
4. **Current probe under-reports.** `scripts/discover.ps1` checks only the
   Dell Command | Monitor classes `DCIM_BIOSEnumeration`/`DCIM_BIOSService`. Both
   are `NotFound` here, so it reports `wmi=false`, `wmi_status=absent` even though the
   built-in provider is present. The owner of `scripts/` should also detect
   `root\dcim\sysman\biosattributes` `EnumerationAttribute`/`IntegerAttribute` and
   `BIOSAttributeInterface` (class definitions, readable unprivileged). An instance read
   returns `NativeErrorCode=AccessDenied` (mapped to `denied`); missing `DCIM_*` classes return `NotFound`.
5. **Heat protection must stay unavailable** on this device until a real
   temperature source is found. No Windows-exposed battery or thermal-zone reading exists.
6. **Cycle count should show as unavailable, not 0.** The firmware reports 0 or
   "-". `BatteryStatus` gives raw charge/discharge rate and voltage, which could
   back the "raw-status adapter" from HANDOFF dead end #12.

### Open questions

1. **Elevated read-only enumeration (needs Patrick's approval to elevate):** list
   `EnumerationAttribute`/`IntegerAttribute` filtered to battery-related names. Record
   names, CurrentValue, PossibleValue, LowerBound/UpperBound/ScalarIncrement,
   ReadOnly, and `PasswordObject.IsPasswordSet` for Admin/System. Still no method calls.
2. Are the denials a namespace security-descriptor policy (Administrators only) or
   a provider-side check? Reading the descriptor needs a method call
   (`__SystemSecurity.GetSecurityDescriptor`), which this read-only pass did not make.
3. Does this BIOS gate `SetAttribute` on an admin password when one is set, and does the
   provider accept a password through a channel that avoids argv/plaintext?
4. Is a battery or skin temperature available elsewhere (e.g. a Dell attribute,
   EC via a supported driver)? None was found through Windows WMI.
5. `BatteryStaticData` failed once with "Generic failure" and then succeeded. Is this
   transient behaviour common enough that the telemetry adapter needs retry handling?

## Dell manual release checklist

- [ ] Record model, BIOS version and signed Dell tool version; omit serial/service tag.
- [ ] Read original mode and thresholds, password-required status and device bounds.
- [ ] Confirm original-state journal survives process/system restart.
- [ ] Set valid Custom 75–80 (if device bounds permit), read back and observe charging stop.
- [ ] Reboot and sleep/hibernate; verify firmware holds cap while app is closed.
- [ ] Test saved-limit top-up, full-charge restoration, service/app crash, offline reboot.
- [ ] Test password absent/correct/wrong without plaintext storage or command-line exposure.
- [ ] Simulate write rejection, partial write and readback mismatch; never report enforcement.
- [ ] Change BIOS externally and update BIOS; detect drift and require reconciliation.
- [ ] Switch users, kill controller, suspend beyond lease; validate ownership and safety behavior.
- [ ] Restore Standard explicitly; uninstall restores original setting and removes service.
- [ ] Test no battery, removed battery, multiple batteries, absent temperature and battery saver.
- [ ] Measure hidden-window CPU and working set for 30 minutes; record actual results.

## Secondary vendor checklist (repeat separately for every model/firmware)

- [ ] Link official API docs, exact version and license constraints.
- [ ] Discover read-only capabilities; prove vendor detection alone cannot enable writes.
- [ ] Establish model/firmware allowlist and dynamic bounds.
- [ ] Verify original-state restoration, rejection handling and readback.
- [ ] Test sleep/reboot/crash/uninstall, credentials and low-battery floor.
- [ ] Record physical charge behavior; keep experimental until reproducible.
