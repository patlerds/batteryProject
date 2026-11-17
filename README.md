# Chargekeeper

An offline Windows battery monitor and charge-policy simulator. React / TypeScript
window, Tauri 2 host, Rust policy core. Blue-white battery theme with gold accents.

**Development build: real hardware is read-only. It does not enforce a charge cap.**
Dell command construction and verification logic are mock-tested; no privileged
service, BIOS writer or BIOS-password storage is shipped. All writable modes run
only in the explicitly labeled simulator. See [report.md](report.md) for the
implementation flow, security review and shipping plan.

## Run on Windows

**Run as your normal Windows user. Administrator access is not required.**
Double-click `target/debug/chargekeeper.exe`, or run this from PowerShell:

```powershell
& 'C:\Users\patrick\Desktop\batteryProject\target\debug\chargekeeper.exe'
```

It monitors battery status and offers unplug reminders. To explore charge-control
modes, expand **Battery simulator** and enable simulation. Running as administrator
does not add firmware limiting: the Dell BIOS writer/service is not implemented.

The app explicitly requests `asInvoker` execution. Its local main window gets
only nine named commands, with no direct core/plugin permissions or remote-page
access. WebView developer tools are disabled. Configuration reads are bounded to
16 KiB and malformed schema versions are rejected.

Prerequisites: Node.js 22+ and npm, stable Rust MSVC, Visual Studio Build Tools
with Desktop development with C++, Windows SDK, and Microsoft Edge WebView2.
Use the x64 Native Tools shell if the compiler cannot find Windows libraries.

```powershell
npm ci
cargo test -p chargekeeper-core --locked
npm run desktop
```

Browser-only visual preview: `npm run dev`. It shows no fake live readings;
interactive device features require the desktop host.

Build the unsigned Windows executable:

```powershell
npm run desktop:build
```

Output: `target/release/chargekeeper.exe`. Close hides the window; use the tray
menu to reopen or quit. The Rust worker samples every 10 seconds even when the
window is hidden. The UI reads the cache every 2 seconds. Simulation advances
10 virtual seconds per sample; it never writes BIOS or real preferences.

The locally verified unsigned developer build is `target/debug/chargekeeper.exe`.
It was built with `npm run tauri -- build --debug --no-bundle` and includes the UI;
it does not require a running Vite server. Twenty-one workspace tests, Clippy,
formatting and frontend build checks passed. A brief startup smoke test passed;
interactive desktop and physical charging scenarios remain unverified.

The secured release executable is now also built at
`target/release/chargekeeper.exe` (about 6.7 MiB), with its SHA-256 checksum in
`target/release/chargekeeper.exe.sha256`. The release passed a startup smoke test;
its embedded Windows manifest was verified to request normal-user execution.
It is unsigned and remains a monitor/simulator preview.

If using the toolchain installed locally during development:

```powershell
. .\scripts\dev-env.ps1
```

If Windows blocks local PowerShell scripts, use the standard installed Rust/Node
tools directly; no machine-wide execution-policy change is required. For this
project-local toolchain, set these variables in the current terminal:

```powershell
$env:CARGO_HOME = Join-Path $PWD '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $PWD '.tools\rustup'
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';C:\Program Files\nodejs;' + $env:PATH
```

## Build and distribute

For a standalone executable, run `npm run desktop:build`. The result is
`target/release/chargekeeper.exe`; no Node.js or Rust installation is required
on the receiving PC. Microsoft Edge WebView2 Runtime must already be installed.
Launch the executable from a normal user session.

For an MSI installer, run `npm run tauri -- build --bundles msi`. Output goes to
`target/release/bundle/msi`. Test install, launch, upgrade and uninstall before
sharing it. Configure Authenticode signing before public distribution; hashes
can detect a changed download but do not authenticate its publisher.

This remains a monitor/simulator preview. Test reminders, tray reopening, startup
and each simulated mode locally before sharing. Physical Dell limiting needs the
unimplemented authenticated service and device validation described in report.md.

## What works

- Real Windows battery enumeration through the `battery` crate (SetupAPI/battery
  IOCTL backend), AC power status, graceful no-battery and missing-sensor states.
- Read-only Dell manufacturer/model/BIOS and interface-presence discovery.
- Software unplug reminders, tray glyph, current-user launch at login.
- Simulator: limit, sailing band, top up, calibration, heat pause, forced discharge.
- Deterministic policy tests, strict future-service protocol, Dell parsing and
  readback/journal contracts tested with mocks.
- Versioned preferences, bounded structured event logs and redacted diagnostics.

Actual battery telemetry availability depends on firmware; missing values are
shown as `—`. No health or temperature estimates are invented.

## Remaining release gates

Dell WMI attribute/bounds discovery and writes, signed authenticated Windows
service, BIOS-password handling, durable top-up recovery and uninstall restoration
are not implemented. Real Latitude charge/reboot tests and performance measurement
are not completed. Secondary vendors have no writable providers.

Read [the research plan](docs/PLAN.md), [device table and manual checklist](docs/DEVICES.md),
[provider guide](docs/PROVIDERS.md), and [contribution instructions](CONTRIBUTING.md).
MIT licensed; dependency licenses remain their authors'. No upstream vendor-control
code was copied. Nothing is published to a remote repository by this build task.
