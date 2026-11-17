# Device evidence and qualification

| Device | Evidence | Status |
|---|---|---|
| Dell Latitude 7320 Detachable, BIOS 1.50.0 | Manufacturer/model/BIOS read by Windows WMI; Monitor WMI and standard-path CCTK absent; initial Win32_Battery query returned no instances | Identified only; charge limiting NOT verified |
| Simulated battery | Automated core integration tests | Simulated, not hardware support |
| Other Dell / Lenovo / ASUS / HP / Surface / Framework / Samsung / MSI / Acer / Huawei | Research references in PLAN | No verified write provider |

No physical laptop is currently claimed as a supported charge-limiting device.

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
