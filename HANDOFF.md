# Chargekeeper — handoff to Claude

**Branch:** `master`  
**HEAD at handoff creation:** `74b0402da661dd851077efc7f534fcddcc96ed82`  
**Repository:** `C:\Users\patrick\Desktop\batteryProject`  
**Snapshot date:** 2026-11-16, America/Los_Angeles  
**Git remote:** none configured at inspection. This is a local repository, not yet verified as uploaded to GitHub.

The SHA above identifies the committed source snapshot. This handoff and the
uncommitted files listed below are additional working-tree state; preserve them.
If you make a handoff/publication commit, its SHA will naturally differ.

## Read first, including the subagent material

1. Read this file, `README.md`, `docs/PLAN.md`, `report.md`, `docs/DEVICES.md`,
   `docs/PROVIDERS.md`, `CONTRIBUTING.md`, and `SECURITY.md`.
2. **Ask Patrick for copies of the subagents' Markdown (`.md`) files, including
   any AGENTS.md, handoff, functionality/UI review, and security review notes.**
   Say that you are a new agent with no access to the previous agents' context.
   Do not assume those files are already in this repository. No subagent Markdown
   files or applicable `AGENTS.md` were found in the project scan. The actual
   subagent in this session was named `/root/ui_review`; it returned chat reports,
   summarized below, but did not provide a known on-disk Markdown report. Do not
   invent missing documents or claim to have read them. If Patrick has only the
   chat reports, ask for those instead and keep doing independent local work.
3. Read `git status --short` before changing anything. Do not discard local work.

## Goal and definition of done

Build **Chargekeeper**, a production-quality, offline **Windows-only** battery
charge limiter, **Dell Latitude first**. An earlier Windows/macOS request was
explicitly superseded: do not build or test a macOS app. Keep the Rust policy and
provider contracts platform-independent for a possible future port.

The desired product sets a default 80% cap with a requested 50–100% UI range,
shows actual battery percentage/state/power/temperature/cycles/health and the
enforcement mechanism, and lives in the Windows tray with a small accessible
window. Hardware capabilities must control which modes are available. Modes:
limit, top-up-once, calibration, heat protection, sailing/start-stop thresholds,
and discharge-to-limit where the device genuinely supports them. Missing sensors
stay unavailable; unsupported hardware gets honest unplug reminders.

Done means all of the following, not merely an executable that starts:

- Real Dell charge capability and valid ranges discovered at runtime. Prefer
  Dell Monitor WMI, then signed Dell Command | Configure (`cctk.exe`) in its
  standard path. Do not invent WMI attribute names, mappings, methods or CLI flags.
- Minimal privileged Windows service; UI remains unprivileged. Restrictive local
  named-pipe ACLs, authenticated/authorized owner/session, bounded typed requests,
  capability/range checks and no generic command/key/register writer.
- Original BIOS state durably recorded before the first change; separate readback
  confirms every write. BIOS password support uses an appropriate protected
  credential mechanism, never plaintext files/logs or process arguments.
- Crash-safe top-up state and recovery; real watchdog/restoration for software
  modes; safe low-battery behavior; lifecycle/drift/multiple-battery handling.
- Physical Latitude tests: 80% limit, AC observation, reboot/sleep persistence,
  top-up recovery, password absent/correct/wrong, external BIOS drift, Standard
  reset, and uninstall restoring the original BIOS state.
- Polished UI/tray, login/notifications, versioned config, bounded redacted logs
  and diagnostic export. No account, telemetry or automatic network traffic.
- Windows build/test/lint CI, tested installer/uninstaller, signing instructions,
  device support evidence and measured idle CPU/RAM. Target idle CPU is <1%; it
  has not been measured. Open-source source/docs/license and honest release notes.

**The production acceptance criteria are NOT met. The current app cannot stop
physical battery charging, even if launched as administrator.**

## Current state

### Implemented and locally checked

- Tauri 2 host in `src-tauri/`, React/TypeScript UI in `src/main.tsx` and
  `src/styles.css`. Blue-white battery theme, gold accents, dark/light colors,
  responsive layout, keyboard labels/focus and reduced-motion handling.
- `src-tauri/src/main.rs::main` starts a Rust worker that calls `poll` every ten
  seconds, independent of a hidden window. `get_status` returns cached data;
  the frontend's `refresh` reads it every two seconds. Closing hides the window;
  use the tray menu to reopen or quit. This intentionally avoids UI timers driving
  battery policy and reminders.
- `src-tauri/src/telemetry.rs::read` uses `battery` 0.7.8's Windows SetupAPI/battery
  IOCTL implementation. `ac_status` calls `GetSystemPowerStatus`. Device enumeration
  is repeated rather than retaining stale battery tags. Sensor values remain
  optional. Real UI workflows and actual telemetry accuracy are not fully tested.
- `src-tauri/src/discovery.rs::probe` runs the embedded, fixed read-only
  `scripts/discover.ps1` once at startup. Hidden PowerShell process, no profile,
  null stdin/stderr, output cap 65,536 bytes and a 15-second timeout. It probes
  manufacturer/model/BIOS, Dell WMI class/method presence and standard CCTK paths;
  **it does not read current charge attributes or execute CCTK**.
- `crates/chargekeeper-core/src/policy.rs::Engine::step` is deterministic. It uses
  monotonic seconds, sailing hysteresis, 3°C thermal hysteresis, a 20% safety floor,
  invalid/stale input checks and interruption handling. Multi-battery safety uses
  the lowest pack; ordinary upper-limit decisions use the highest pack.
- `crates/chargekeeper-core/src/simulator.rs::Simulator` implements
  `BatteryProvider` and `ChargeController`. `Simulator::step` runs the policy and
  changes only in-memory battery/firmware state. The worker advances ten virtual
  seconds per call; unit tests can advance it rapidly. Full-hold calibration is
  600 virtual seconds, followed by discharge to 25%, then recharge. Simulated
  charge/discharge changes by 1.5 percentage points per tick. No hardware writes.
- `main.rs::save_config` validates config in Rust and rejects unsupported modes
  outside simulation. `set_simulation` reloads real preferences, clears cached
  readings and creates a fresh simulator/engine. Simulation does not save its
  settings over actual preferences. `restore_defaults` works in simulation only.
- Software reminder notifications, dynamic tray glyph, current-user login setting
  through `set_launch_at_login`/`get_launch_at_login`, fixed-URL `open_dell_support`,
  and selected-field `export_diagnostics` are implemented. Startup smoke testing
  does not establish that all these interactive features work end to end.
- `storage.rs::{decode_config,load,save}`: schema 1 JSON, migration from schema 0
  or absent schema, 16 KiB read/parse cap, malformed/future schema rejection,
  unknown-field rejection, flushed temp-file replacement for UI preferences.
  Corrupt/future config is preserved and UI saving is blocked. This is NOT a
  production privileged BIOS-state journal.
- `src-tauri/src/logging.rs::event`: static event names only, local JSON lines,
  rotation after 256 KiB with one previous file; no raw hardware dumps/secrets.
- Local checks passed: **21 tests (20 core + 1 telemetry-adapter), Clippy with
  warnings denied, rustfmt check, TypeScript checking and Vite production build**.
- Unsigned standalone debug and optimized release executables built. Both secured
  builds passed five-second hidden startup/process-liveness smoke tests and were
  stopped afterward. Windows SDK Manifest Tool verified embedded `asInvoker`.
  No interactive desktop automation or independent penetration test was performed.

### Hardware evidence — do not overstate it

- Read-only WMI identified **Dell Inc., Latitude 7320 Detachable, BIOS 1.50.0**.
- The initial `Win32_Battery` query returned no instances. This does not prove
  that the machine has no battery; IOCTL telemetry needs its own verification.
- The saved probe returned `wmi=false`, `cctk=false`: Dell Monitor WMI was not
  available/detected and no CCTK was found in the two standard paths. Access
  denial and absence must remain distinguishable uncertainties where relevant.
- No BIOS setting or hardware register was changed anywhere in this session.
- `main.rs::get_status` intentionally returns `Capabilities::default()` for
  physical hardware, **even if Dell software is installed later**. Merely running
  as admin or installing CCTK will not turn this build into a hardware limiter.

### Missing, incomplete or unverified

- No Windows service crate/binary, installer registration, pipe implementation,
  real IPC authentication or operational watchdog exists.
- `protocol.rs::Request::{decode,validate}` and `Lease::{renew_authenticated,expired}`
  are tested future-service building blocks only. Messages are capped at 4,096
  bytes; lease duration is 30 seconds. No running component enforces the lease.
- `dell.rs::{parse_cctk,cctk_args,apply_verified}`, `Bounds::validate`, `BiosConfig`,
  `DellTransport` and `Journal` are a pure adapter/transaction contract with mocks.
  No concrete WMI/CCTK transport, attribute/value/bounds discovery, password path,
  rollback implementation or durable original-state journal is wired to the app.
- The real telemetry adapter is currently a function facade, not a concrete
  `BatteryProvider` implementation; the traits are currently realized by the
  simulator. Additional WMI battery telemetry/sensor supplementation is missing.
- Hardware modes, durable top-up recovery, firmware drift reconciliation,
  fast-user-switch ownership and low-power/sleep/hibernate behavior need real
  implementation/qualification. Some policy safeguards are tested in isolation.
- `scripts/uninstall-preferences.ps1` only prints current-build cleanup guidance;
  it is not the required future service/BIOS-restoring uninstaller.
- MSI/NSIS packaging commands/scripts exist, but installers, upgrade/uninstall,
  Authenticode signing, hosted CI and performance targets have NOT been exercised.
- No secondary vendor has a writable provider. Do not add speculative EC/ACPI
  access to make the device matrix look complete.
- Runtime reminders, login registration, diagnostics UI action, tray reopening and
  each simulated mode still need an actual interactive app walkthrough.

## Security decisions already made

- `src-tauri/windows.manifest`: `asInvoker`, `uiAccess=false`, Common Controls v6.
  The app does not request administrator privileges. **asInvoker inherits the
  parent's token; it does not drop rights if launched from an admin shell.**
- `src-tauri/build.rs::main` registers all nine custom commands in
  `tauri_build::AppManifest::commands`, so Tauri's ACL applies to them.
- `src-tauri/capabilities/main.json` grants only these commands to the local
  `main` window: `get_status`, `save_config`, `set_simulation`,
  `set_simulated_battery`, `restore_defaults`, `export_diagnostics`,
  `set_launch_at_login`, `get_launch_at_login`, `open_dell_support`.
  Broad `core:default`, `notification:default`, `autostart:default` grants were
  removed. The frontend uses custom `invoke` calls; native Rust plugin calls
  still implement notification/startup/tray behavior. No remote URL gets a grant.
- `src-tauri/tauri.conf.json`: restrictive CSP; `devtools=false`, including the
  debug preview. Disabling devtools is hardening, not an XSS defense or a claim
  that a compromised renderer is trustworthy. Inline styles are used by the UI.
- No frontend filesystem/shell plugin or generic executable/attribute input.
  Dell help uses a fixed HTTPS URL through `ShellExecuteW` only on user action.
- Keep the UI unprivileged when adding the service. Add real caller authentication
  and owner authorization; JSON validation and an executable signature alone are
  not a complete service security boundary. Never put BIOS passwords in argv.
- Do not claim the app is unhackable. No Rust advisory scan, fuzz campaign or
  independent security audit has been completed. A checksum is not a signature.

## Git state, publication and interrupted work

Committed history, oldest first:

| Commit | Purpose |
|---|---|
| `dc16801` | Dell research plan and initial ignore rules |
| `e511cb4` | Rust policy, simulator, Dell/protocol contracts, lockfile/tests |
| `3a9b73d` | Windows Tauri host, UI, icons and frontend toolchain |
| `09920ae` | Report, README, provider/device docs, CI, packaging scripts |
| `74b0402` | Explicit command ACL, asInvoker manifest, bounded config parsing |

Uncommitted before writing this file:

- `README.md`: normal-user launch, secured release executable/checksum, current
  test count, standalone/MSI commands and distribution limitations.
- `report.md`: follow-up security design/review, 21 tests, release build and startup
  verification, privilege nuance and checksum/signing distinction.
- `src-tauri/permissions/autogenerated/`: nine generated `.toml` files for
  `get_status`, `save_config`, `set_simulation`, `set_simulated_battery`,
  `restore_defaults`, `export_diagnostics`, `set_launch_at_login`,
  `get_launch_at_login`, `open_dell_support`. Generated from build.rs, not handwritten;
  currently untracked, and intended to be included in the documentation/ACL checkpoint.
- `HANDOFF.md`: this new file.

The attempted final commit was **not executed**: automatic approval review hit
an account usage limit and could not complete. This was a review failure, not a
finding that the commit was unsafe. Do not bypass approval restrictions. Recheck
whether approval is available before retrying a necessary `.git` mutation.

Patrick's latest additional instruction: **“this should be on my github.”**
Treat source publication as requested, while preserving the handoff task.

- `git remote -v` returned no remote. `gh` was not found on PATH.
- The connected GitHub profile is `patlerds`; `github.list_repositories` returned
  an empty accessible list. This does NOT prove the account has no repositories;
  connector access may simply not include them.
- The available GitHub connector tools can read/write existing repositories but
  expose no repository-creation operation. No GitHub repository, push or release
  has been created by this session.
- A question was sent asking Patrick for the target repository URL and, if new,
  to create an empty repository and grant connector access. No answer was available
  when writing this snapshot. Do not guess a destination or overwrite an existing
  repository. Ask for public/private visibility if creating a new repository.
- After an authorized destination is known, review and commit the pending source
  files, configure the remote, inspect remote history/default branch, then push
  without force. Keep `master` until a branch decision is made; do not silently
  replace another project's history or force a `main` migration.
- Publish source, lockfiles and docs. `.gitignore` excludes `.tools/`, `target/`,
  `dist/`, `node_modules/`, `artifacts/`, logs and signing-key formats. Never add
  credentials, diagnostics or local browser profiles. Executables belong in a
  consciously created prerelease/artifact, not git source history.
- `README.md`/`report.md` currently correctly say no remote publication occurred;
  update those statements only after a real push succeeds.

## Files touched during this session and why

Paths below are relative to the repository root. This covers the original build
and the subsequent security/executable follow-up, not just the last documentation turn.

| Paths | Purpose / concrete entry points |
|---|---|
| `Cargo.toml`, `Cargo.lock`, `crates/chargekeeper-core/Cargo.toml` | Rust workspace, serde dependencies, reproducible resolution, size-focused release profile |
| `crates/chargekeeper-core/src/lib.rs` | `Capabilities`, `Battery`, `Snapshot::{lowest,highest,valid}`, `BatteryProvider`, `ChargeController` |
| `crates/chargekeeper-core/src/policy.rs` | `Config::validate`, `Engine::{step,cancel_temporary_mode}`, `Mode`, `Phase`, `Decision`, policy regressions |
| `crates/chargekeeper-core/src/simulator.rs` | `Simulator::{step,reset_policy,restore_defaults}`, simulated telemetry/controller and integration tests |
| `crates/chargekeeper-core/src/dell.rs` | Device bounds, strict CCTK parser/args, mock transport and journal/readback transaction tests |
| `crates/chargekeeper-core/src/protocol.rs` | Strict future-service request contract, capability checks and monotonic lease tests |
| `crates/chargekeeper-core/src/storage.rs` | Versioned UI config, migration, bounded reads and malformed-version rejection |
| `src/main.tsx` | `App`, `refresh`, `action`, `save`, `toggleSimulation`, `Metric`, `ModeButton`, `ToggleRow`; real/preview/simulator separation and controls |
| `src/styles.css` | Blue-white/gold battery scene, light/dark, responsive/keyboard styling; corrected inherited dark text |
| `index.html`, `vite.config.ts`, `tsconfig.json` | Local frontend entry, fixed localhost development port 1420, strict TypeScript/build configuration |
| `package.json`, `package-lock.json` | React/Tauri/Vite dependencies and build/run/lint scripts |
| `src-tauri/Cargo.toml` | Tauri, notification/autostart plugins, battery crate and Windows API features |
| `src-tauri/build.rs`, `src-tauri/windows.manifest` | Custom-command ACL generation and explicit normal-user Windows execution |
| `src-tauri/tauri.conf.json`, `src-tauri/capabilities/main.json` | Window/bundle/CSP/devtools configuration and exact local command allowlist |
| `src-tauri/permissions/autogenerated/*.toml` | Generated allow/deny permissions for the nine commands; currently pending commit |
| `src-tauri/src/main.rs` | Cached state, worker, typed UI commands, tray/menu, preferences, diagnostics and current-user startup |
| `src-tauri/src/telemetry.rs` | `read`, `ac_status`, `reported_state`, `reported_rate`, dependency ambiguity regression test |
| `src-tauri/src/discovery.rs`, `scripts/discover.ps1` | Bounded fixed read-only hardware/interface discovery |
| `src-tauri/src/logging.rs` | `event` bounded selected-field local logs |
| `assets/icon.svg`, `src-tauri/icons/*` | Code-native battery icon source and Windows ICO/PNG resources; no macOS app was built |
| `scripts/dev-env.ps1` | Optional local Rust/Node PATH setup; Windows execution policy may block dot-sourcing |
| `scripts/package.ps1` | MSI/NSIS development packaging wrapper; not yet executed as installer qualification |
| `scripts/uninstall-preferences.ps1` | Current read-only build cleanup guidance, not service/BIOS restoration |
| `.github/workflows/windows.yml` | Windows frontend/Rust lint/tests/build pipeline, pinned action commits, read-only token |
| `.gitignore` | Exclude build/toolchain/private signing artifacts |
| `docs/PLAN.md` | Official Dell/vendor sources, architecture/capabilities, phased release gates |
| `docs/DEVICES.md` | Honest device evidence and physical qualification checklists |
| `docs/PROVIDERS.md` | Provider-extension rules and verification requirements |
| `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE`, `report.md` | Run/build/contribution/security/license material, implementation/security/shipping report |

Generated/ignored local artifacts also exist: `.tools/`, `node_modules/`, `dist/`,
`target/`, `src-tauri/gen/`. They are not source to upload. Headless browser preview
screenshots are `.tools/ui-review.png` and `.tools/ui-review-final.png`. The local
HTTP preview server and test app processes started by the agent were stopped.

## Decisions and reasoning — do not relitigate without new evidence

- Tauri 2 + React + Rust was the requested architecture and is already working.
  Keep this lean; Patrick explicitly requested “lazy senior dev / ponytail” mode:
  smallest complete change, no speculative dependencies, concise explanations.
- Windows/Dell replaces cross-platform scope. Do not add macOS implementation.
- Original delivery used the user's maximum three draft/review iterations and
  stopped honestly with gaps. Later security/executable/handoff requests were
  follow-up work. Do not represent unfinished M2–M5 as completed milestones.
- Patrick explicitly requested subagent functionality and clean UI review. One
  subagent implemented/reviewed `src/main.tsx`/`src/styles.css`, then reviewed Rust
  functionality and security compatibility. Ask for its Markdown/chat material.
- Capabilities default false on real hardware because no qualified writer exists.
  Vendor string, interface presence or a successful mock does not prove support.
- Dell's published Custom ranges are start 50–95%, stop 55–100%, gap >=5; these
  documentation facts are cited in PLAN, not used as guessed live device bounds.
  A 50% physical stop cannot be promised through this documented Dell interface.
- `BiosConfig::Standard` is an explicit user-selected reset. Future uninstall must
  restore the recorded ORIGINAL BIOS mode, which could instead be Adaptive/Custom.
- Firmware caps can persist while the app is closed; software modes require a
  real authenticated lease/watchdog. Do not apply the original universal-reset
  requirement in a way that destroys the newer Dell firmware-persistence design.
- Do not pass BIOS passwords on a CCTK command line, even if a flag exists. Verify
  a secure vendor-supported path before enabling password-required writes.
- Read-only discovery still uses a fixed PowerShell script; no new generic script
  execution surface was exposed. App writes only per-user preferences/diagnostics,
  notification/login preferences, or simulator state in the current implementation.
- Bare `cargo build` is not the standalone packaging command: in debug/dev mode
  it uses the Vite URL. Tauri CLI build embeds the frontend assets.
- No automatic updater was added. No account or backend service is needed for
  the current UI. MIT license is included; vendor-control source was not copied.
- `lto=true`, `opt-level="s"`, `codegen-units=1`, `strip=true` are in Cargo.toml.
  First optimized Windows build took **11m19s** on this Latitude. Quiet optimization
  was CPU-active, not a failed build; avoid restarting it just because output pauses.

## Dead ends and resolved failures

1. **No tools on PATH:** Node was installed at `C:\Program Files\nodejs`, but the
   original process PATH omitted it; Rust was absent. Rust stable 1.100.0 MSVC,
   rustfmt and Clippy were installed locally in `.tools/`. Use the setup below.
2. **Sandbox networking:** initial npm/crates.io/rustup requests failed with
   EACCES/connect errors. Approved execution outside the sandbox fetched dependencies.
   Do not work around required approval by extracting credentials or using another
   tool as a hidden network tunnel. npm's initial audit reported zero vulnerabilities;
   that is not a substitute for a current full security review.
3. **PowerShell policy:** `. .\scripts\dev-env.ps1` and `& .\scripts\discover.ps1`
   were blocked by the user's execution policy. Do not change machine-wide policy.
   Set environment variables directly; the app embeds its fixed discovery script
   and runs it with `-Command`, not a user-supplied script/file endpoint.
4. **WMI sandbox denial:** `Get-CimInstance` initially returned access denied in
   the sandbox. An approved read-only host probe worked and identified the Latitude.
5. **Git permissions/ownership:** `.git` is read-only in the coding sandbox.
   Approved host git initially saw “dubious ownership” because sandbox and host
   users differ. `git -c safe.directory=C:/Users/patrick/Desktop/batteryProject ...`
   fixed that invocation without weakening global git configuration.
6. **Final commit approval failure:** automatic review later hit a usage limit;
   pending docs/permission files were not committed. This was not a security denial.
7. **Cargo cache/build locks:** overlapping stalled builds held locks. Only
   identified project Cargo processes were stopped. Serialize dependent build
   operations; do not kill unrelated compilers or clear build caches casually.
8. **Serde unknown fields:** unit enum `GetStatus` accepted extra JSON fields
   despite the enum annotation. `Request::GetStatus {}` / `RestoreDefaults {}`
   empty struct variants fixed it. `rejects_untrusted_inputs` is the regression.
9. **UI-driven sampling:** initial `get_status` advanced simulation and read
   hardware on every UI request. Hidden windows can throttle timers. Moved all
   sampling/policy work into `main.rs::poll` background worker; UI now reads cache.
10. **Simulation stuck low:** `Simulator::restore_defaults` initially left
    `charging=false` after reaching a cap. It now sets `charging=true`; regression
    `low_battery_restoration_resumes_charging_after_holding` covers the safety floor.
11. **Calibration stuck in FullHold:** stale data/lost AC cleared its timer without
    changing phase. `Engine::cancel_temporary_mode` now completes/cancels the
    temporary mode; regression `interrupted_full_hold_cancels_instead_of_stalling`.
12. **Misleading Windows dependency telemetry:** battery 0.7.8 converts unknown
    IOCTL rate to zero, and idle AC to `State::Full` even below 100%. `reported_rate`
    maps zero/nonfinite rate to None; `reported_state` labels Full only at >=99.5%.
    Both cases are in `no_invented_zero_rate_or_full_state`. Actual zero power
    therefore also displays unavailable until a raw-status adapter is added.
13. **Desktop Dell help link:** a normal HTML external anchor was unreliable in
    Tauri. It now invokes the fixed-URL `open_dell_support`; install guidance only
    appears for Dell identification, not every vendor.
14. **Missing icon:** Windows Tauri resource build failed on absent `icons/icon.ico`.
    Created `assets/icon.svg`, generated Windows resources with the Tauri CLI.
    Removed generated non-Windows icon outputs; no macOS build/test was performed.
15. **Vite child access denial:** direct frontend build could work while Tauri's
    nested `beforeBuildCommand` failed reading `../..` under the sandbox. The same
    Tauri build succeeded with approved host access; it was not a source import bug.
16. **Screenshot review:** headless Brave was initially blocked by sandbox GPU
    permissions. Approved headless local preview capture worked. Screenshot exposed
    dark text inheritance; `:root` now uses `color:var(--text)` and final image was
    inspected. This was browser preview, not a screenshot of live native telemetry.

## Exact commands: build, run and test

Run from a normal PowerShell session. No admin rights are required to run the
app. Host execution outside the coding sandbox is not equivalent to needing an
administrator UI. Future service installation/BIOS writes need their own design.

### Existing workstation environment

```powershell
Set-Location -LiteralPath 'C:\Users\patrick\Desktop\batteryProject'
$env:CARGO_HOME = Join-Path $PWD '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $PWD '.tools\rustup'
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';C:\Program Files\nodejs;' + $env:PATH
node --version
cargo --version
```

This local `.tools` directory is ignored and will not accompany a GitHub clone.
On a fresh machine install Node 22+, Rust stable x64 MSVC with rustfmt/Clippy,
Visual Studio C++ Build Tools + Windows SDK, and Edge WebView2 Runtime. Use global
Rust normally there instead of pointing CARGO_HOME/RUSTUP_HOME at nonexistent
project-local installations. An x64 Native Tools terminal can help SDK discovery.

### Dependencies and checks

```powershell
npm ci
npm run lint
npm run build
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
```

The `--offline` commands passed on this workstation's populated Cargo cache. For
a fresh cache, fetch approved dependencies first or omit `--offline`. `--locked`
preserves Cargo.lock. Frontend `lint` is TypeScript checking, not ESLint. Do not
re-run the eleven-minute release build for a docs-only change.

### Run already-built standalone executables

```powershell
& 'C:\Users\patrick\Desktop\batteryProject\target\release\chargekeeper.exe'
# Or the secured standalone developer preview:
& 'C:\Users\patrick\Desktop\batteryProject\target\debug\chargekeeper.exe'
```

Both embed the UI and need no Node/Rust/development server on a receiving PC.
WebView2 Runtime must be installed. Use **Battery simulator** to exercise modes;
physical hardware remains reminder-only. Close hides; tray **Quit** exits.

### Develop or rebuild

```powershell
npm run desktop
# Browser visual preview only; no live battery or working desktop commands:
npm run dev
# Standalone developer executable, embedded frontend:
npm run tauri -- build --debug --no-bundle
# Optimized standalone executable, embedded frontend:
npm run desktop:build
```

The two final build commands produced `target/debug/chargekeeper.exe` and
`target/release/chargekeeper.exe`. The optimized file in this snapshot is
**6,986,240 bytes**, unsigned, with SHA-256:

```text
2b23a787a1312071aae358621c862141cb28983a86611da42c2ebd5cd56bd1fb
```

Sidecar: `target/release/chargekeeper.exe.sha256`. This identifies this exact
local artifact; rebuilding or signing will change the hash.

```powershell
Get-FileHash -LiteralPath target\release\chargekeeper.exe -Algorithm SHA256
```

### Packaging, not yet verified

```powershell
npm run tauri -- build --bundles msi
# Alternative Windows installer:
npm run tauri -- build --bundles nsis
```

Expected directories: `target/release/bundle/msi` and `target/release/bundle/nsis`.
`scripts/package.ps1 -Format msi` wraps npm ci + Tauri packaging, but may be blocked
by local PowerShell script policy. MSI/NSIS toolchain downloads may need network
approval. Choose/test an offline WebView2 runtime installation strategy before
claiming an offline installer. These packages currently have no service or BIOS
restoration hook. Configure real Authenticode signing before public distribution.

### Verify embedded normal-user manifest (SDK command actually used)

```powershell
& 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\mt.exe' `
  '-inputresource:C:\Users\patrick\Desktop\batteryProject\target\release\chargekeeper.exe;#1' `
  '-out:C:\Users\patrick\Desktop\batteryProject\.tools\release-app.manifest'
[xml]$releaseManifest = Get-Content -Raw -LiteralPath .tools\release-app.manifest
$releaseManifest.assembly.trustInfo.security.requestedPrivileges.requestedExecutionLevel.level
# Expected: asInvoker
```

The five-second smoke checks launched only the built test process with
`Start-Process -WindowStyle Hidden -PassThru`, checked `HasExited`, then stopped
that exact PID. They were liveness checks, not assertions that the full UI works.

## Next steps, in priority order

1. **Finish the requested GitHub handoff/publication:** get the destination URL
   and connector/repository access, ask for the subagents' Markdown/chat reports,
   inspect pending docs/generated permissions, commit them and this file, then
   push without overwriting remote history. No build outputs/secrets in source.
2. **Interactive preview acceptance:** launch the release executable normally;
   verify all nine allowlisted commands through actual UI flows, simulator modes,
   exported redaction, login toggle, hidden-window reminders and tray reopening.
   Record real evidence in `docs/DEVICES.md`/`report.md`, not simulated claims.
3. **Unblock Dell discovery:** with Patrick's installation authorization, install
   appropriate official Dell software if absent. Read attributes, service method
   metadata, allowed values/ranges/password status; do not change BIOS while merely
   probing. Consult official sources in PLAN.md and verify current device metadata.
4. **Build the real minimal service:** authenticated local IPC, exact command
   surface, owner/session enforcement, restrictive ACLs and protected journal.
   Wire a concrete Dell transport only after bounds/credentials/rollback are
   understood. Keep a hard readback gate before reporting enforcement.
5. **Implement original-state and travel recovery:** persist before writes,
   restart recovery, software-mode watchdog, transactional/partial-failure
   behavior and drift handling. Do not confuse UI preferences with the journal.
6. **Physically qualify the Latitude:** complete `docs/DEVICES.md` on AC, reboot,
   sleep/hibernate, top-up, password/error paths, Standard and uninstall restoration.
   Confirm a battery is actually present/readable and observe real charging.
7. **Package/release safely:** exercise MSI or NSIS install/upgrade/uninstall on
   clean Windows, measure CPU/RAM, review dependency advisories/licenses and
   service security, enable hosted CI, sign/checksum artifacts and publish an
   explicitly labeled prerelease. Update SECURITY.md with a real private reporting
   URL and owner. Broaden vendors only after Dell works and device evidence exists.

## Open questions for Patrick

Ask concisely; keep useful local work moving without waiting on optional answers.

- What is the destination GitHub repository URL? If new: desired name and public
  or private visibility? The connected account is `patlerds`; grant repository
  access to the connector or provide another authorized publishing route.
- **Please provide the subagents' `.md` files (or their chat reports if no Markdown
  files exist), including AGENTS.md/review/handoff instructions for Claude.**
- May the next agent install official Dell Command | Monitor/Configure if needed
  and perform the physical charging/reboot tests? Coordinate disruptive tests and
  actual firmware changes with Patrick; the prior session performed none.
- Is a BIOS setup/admin password configured? Do not send the password into chat,
  git, logs or CLI arguments. The secure credential/UI path must be implemented
  before requesting entry into the application.
- For public release, who owns the publisher/signing identity and certificate,
  and the vulnerability-reporting/support channel? No certificate has been supplied.
- Is the next deliverable a shareable monitor/simulator prerelease, or should
  publication wait until real Dell limiting is qualified? Both paths must label
  capabilities honestly. The latest explicit requests were `.exe`, security,
  source on GitHub, and this Claude handoff.
