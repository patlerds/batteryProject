# Changelog

All notable changes to Chargekeeper are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

**Status:** Chargekeeper is currently a battery monitor and simulator.
Physical Dell charge limiting is still **not implemented**, and no hardware
support has been verified on a real device. All Dell behaviour is tested
against mocks only.

## [Unreleased]

### Added

- Critical-heat stop (C5): at 50 C or more the engine never charges, even
  below the 20% safety floor and in every mode, and never force-discharges.
  It latches until the battery falls below 45 C. Below the critical level the
  floor still beats normal heat protection. A `heat_c` setting above the
  critical stop is rejected.
- Decision notices (`top_up_started`, `top_up_complete`, `critical_heat`),
  edge-triggered and emitted even when the device has no hardware capabilities.
- Desktop toasts for these notices with bounded retry. The critical-heat toast
  is shown even when notifications are off and jumps the queue.
- Status now exposes `notice`, `notice_id` and `active_critical_heat`. The UI
  has a dismissible top-up banner, a non-dismissible red critical-heat banner,
  readable reason text, and a note when the device reports no battery temperature.
- Discovery detects Dell's built-in BIOS WMI provider and reports instance
  read status (present, denied, absent, error) separately. A cycle count of 0
  is reported as unavailable, and a `temperature_available` flag was added.
- `scripts/dell-read-elevated.ps1`: a strictly read-only script, run by the
  owner and never by the app, that queries battery/charge BIOS attributes and
  the password-set status. It includes a self-test.

### Fixed

- Calibration no longer asks the battery to stop charging and force-discharge
  at the same time; discharge is allowed only in the limit and complete phases (C1).
- Top-up and calibration now finish at 99% or when the battery reports Full,
  with time limits (4 h per phase, 12 h per mode). Previously a pack that
  settled at 99% could keep the limit disabled forever (C3).
- The 20% safety floor now latches at 20% and releases at 25%. With several
  packs, the floor protects the lowest pack, and top-up no longer deadlocks
  when packs sit at different levels (C4).
- After a top-up, the engine no longer drains the battery back to the limit
  automatically while on AC (C2).
- The "please unplug" reminder re-arms when AC is lost or the limit changes,
  so an unplug at 80% and replug at 77% now reminds again (H3).
- The reminder also re-arms below the band when the engine returns early for
  the safety floor, an unsupported mode or heat protection (C14).
- The Dell contract records the original BIOS setting once (first write wins)
  and verifies a rollback if a write fails (C7).
- Dell CLI output such as `+75` or `075` is now rejected instead of accepted (C9).
- Config files that start with a UTF-8 BOM are now read correctly, and files
  with duplicate top-level keys are rejected (C13).
- The simulator applies the documented Dell bounds, and the band minimum in
  the UI matches what the backend accepts.
- Discovery now tells "WMI access denied" apart from "WMI absent" (H6).

### Security

- Login autostart is always registered for the current user (HKCU) with a
  quoted path. Before, running from an admin shell could register an unquoted
  path for all users (H1). Known limit: checking or disabling autostart still
  looks at HKLM.
- Reminder delivery errors are now observed, retried up to 3 times and logged
  instead of being hidden. Windows may still silently drop a toast from an
  unregistered app, which needs an installed-build test (H2).
- Only one instance of the app can run, via a named mutex, which prevents
  duplicate reminders and racing writes (H4).
- The documented Dell limits (start 50-95, stop 55-100, gap of at least 5) are
  enforced in production code, and discovered bounds can only narrow them (C8).
- Settings that Dell cannot express are rejected, including from
  `save_config` and the simulation command (C6).
- The protocol refuses to inhibit charging or force-discharge at or below the
  floor, or when the charge level is unknown. The future service must still
  re-check on every tick (C10).
- Config saves use a unique temp file, flush to disk and clean up on any
  failure, so a crash cannot leave a corrupt or shared temp file (C12).

### Changed

- Replaced `tauri-plugin-autostart` and `tauri-plugin-notification` with direct
  use of `auto-launch` 0.6 and `notify-rust` 4.18. Both were already in the lockfile.
- Top-up now stays full after it completes and the user is notified, instead
  of draining back to the limit (owner decision on C2).
- Config validation now depends on the device's capabilities (`validate_for`).
- `Journal` gained a required method, and `Request::validate` gained an
  argument. Neither is used by the desktop app yet.

### Docs

- Added `CLAUDE_REVIEW.md`: an independent review with finding IDs (C1-C14,
  H1-H12), verified claims, open questions and a proposed work split.
- Open items not fixed here: heat protection versus the safety floor (C5),
  lease owner identity (C11), worker supervision (H5), start hidden in the
  tray (H7), a hard-coded PowerShell path (H8), plus H9-H12.
- Added a read-only raw discovery of the Latitude 7320 to `docs/DEVICES.md`
  (M-C): Dell's built-in BIOS WMI provider is present, but charge attributes
  and password status are access denied to a normal user.
- The Latitude 7320 reports no battery temperature (`BatteryTemperature` has
  0 instances), so critical-heat protection cannot trigger on that device.
- C5 is now fixed (see above); it is no longer an open item.
