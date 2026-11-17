# Chargekeeper: independent review by Claude

**Reviewed:** `master` at `4d93635` (HANDOFF.md was written at `74b0402`)  
**Date:** 2026-11-17  
**Milestone commit:** `64c20fe`  
**Method:** Claude coordinated the work. Two read-only Opus reviewers covered the core crate and the host/UI. Two Opus fix workers had disjoint file ownership, and a Sonnet worker handled a small follow-up. Claude spot-checked the key findings (H1, C1, C7) in the source.  
**Scope limits:** BIOS settings were not changed, cctk was not executed, nothing ran elevated, nothing was installed and no GUI was driven. Nothing here claims hardware support.

The subagent Markdown and chat reports that HANDOFF asks for were **not provided**. The user asked for an unattended run with no questions. See the open questions below.

---

## 1. Verdict

The app is an honest **monitor and simulator**. On real hardware it reads telemetry, shows reminders and manages login autostart. `get_status` returns `Capabilities::default()` for real hardware, so it cannot stop charging. The UI never claims otherwise.

The architecture is sound: a pure policy core, a narrow Tauri command ACL, an `asInvoker` manifest, and a Dell transport that is still only a contract. However, the policy engine and the Dell contract had real safety bugs that HANDOFF's wording overstated. They would have mattered as soon as a real writer was wired in. Milestone M-B, below, fixed them.

## 2. What works (verified)

| Claim | Evidence |
|---|---|
| Exactly 9 commands are allowlisted, with no core or plugin grants | `src-tauri/build.rs`, `capabilities/main.json` and the `invoke_handler` match; the reviewer cross-checked them |
| CSP is tight and devtools are off | `src-tauri/tauri.conf.json` |
| `asInvoker` manifest | `src-tauri/windows.manifest` |
| Real hardware has no capabilities | `main.rs::get_status` (now through `effective_caps(false)`) |
| Discovery is read-only, fixed and bounded | `discovery.rs::probe` and `scripts/discover.ps1`: no user data reaches the command line |
| Diagnostics export cannot path-traverse | `main.rs::export_diagnostics`: fixed file, field allowlist |
| Dell attribute names match Dell's published reference | `dell.rs` |
| Checks before the milestone | 21 tests (20 core + 1 host), with clippy `-D warnings`, fmt, tsc and the Vite build all passing |

## 3. Findings and status

Severity: H = High, M = Medium, L = Low. Status ✅ means fixed in `64c20fe` with a regression test, except where marked otherwise.

### Core: `crates/chargekeeper-core`
| ID | Sev | Location | Finding | Status |
|---|---|---|---|---|
| C1 | H | `policy.rs::Engine::step` | Calibration FullHold asked for inhibit **and** force-discharge at the same time when `discharge` was on | ✅ discharge is allowed only in the Limit and Complete phases |
| C3 | M | `policy.rs` | Top-up and calibration finished only at exactly 100%. A pack that settles at 99% stayed in top-up forever, which disabled the limit | ✅ "full" means ≥99% or reported Full; 4 h per phase, 12 h per mode |
| C4 | M | `policy.rs` | The 20% floor had no hysteresis, and the hold followed the highest pack, so with several packs the lowest could sit at the floor. Top-up deadlocked with packs at 100% and 90% | ✅ floor latches at 20% and releases at 25%; floor beats hold on the lowest pack |
| C2 | M | `policy.rs` | After a top-up, the engine immediately drained back to the limit while on AC | ✅ conservative fix: never drains automatically after a temporary mode |
| H3 | M | `policy.rs` (`reminded`) | The reminder didn't re-arm on unplug or limit change, so an unplug at 80% and replug at 77% gave no reminder | ✅ |
| C8 | M | `dell.rs::Bounds` | The documented bounds (start 50–95, stop 55–100, gap ≥5) existed only in tests, and discovered bounds were trusted as-is | ✅ `DOCUMENTED_BOUNDS`; discovery can only narrow them |
| C7 | M | `dell.rs::apply_verified` | The "original" setting was re-recorded on every call and there was no rollback. The test only passed because the mock kept the first value | ✅ first write wins and rollback is verified; tested with a journal mock that overwrites on every call |
| C6 | M | `policy.rs::Config::validate` | Accepted limit/band pairs that Dell can't express, and the simulator applied them anyway | ✅ `validate_for(caps)`, also wired into `save_config` and `set_simulation` |
| C10 | M | `protocol.rs::Request::validate` | Clients could turn on inhibit or force-discharge with no floor check | ✅ refused at or below 20% or when unknown. The future service must still re-check on every tick |
| C9 | L | `dell.rs::parse_*` | Accepted `+75` and `075` | ✅ |
| C5 | L/M | `policy.rs` | The safety floor overrides heat protection | open, needs a product decision (Q5) |
| C11 | L | `protocol.rs::Lease` | No owner identity | open, belongs to the service milestone |
| C12, C13 | L | `storage.rs` | Fixed temp file name and no write-through; BOM rejected and duplicate keys accepted | open |
| C14 | L | `policy.rs` | `reminded` isn't reset on some other paths | mostly covered by the H3 fix; re-verify |

### Host: `src-tauri`, `scripts`, `src`
| ID | Sev | Location | Finding | Status |
|---|---|---|---|---|
| H1 | M | `main.rs` (autostart) | `tauri-plugin-autostart` uses `auto-launch`'s `Dynamic` mode, which tries **HKLM first**. Launched from an admin shell, it registered the exe for all users, with an unquoted path. Claude confirmed this in the `auto-launch-0.6.0/src/windows.rs` source | ✅ calls `auto-launch` directly with `CurrentUser` and a quoted path. Remaining limit: `is_enabled` and `disable` still look at HKLM |
| H2 | M | `main.rs` (reminders) | The notification plugin hid errors, and a reminder was marked sent even when it failed | ✅ uses `notify-rust` directly; bounded retry (3); logs `reminder_failed`; status has `reminder_failed` (no UI yet) |
| H4 | L | `main.rs::main` | No single-instance guard, so duplicate reminders and racing writes were possible | ✅ `Local\` named mutex |
| H6 | L | `discover.ps1` | Couldn't tell "access denied" from "absent" | ✅ `wmi_status` field |
| H5 | L/M | `main.rs` worker | The worker isn't supervised; a panic while holding the lock poisons every command | open |
| H7 | L | `tauri.conf.json` | The window opens at every login instead of staying in the tray | open |
| H8 | L | `discovery.rs` | Hard-coded PowerShell path. **This probe must never be reused in a privileged service** | open, note for the service milestone |
| H9–H11 | L | various | A compromised renderer could spam the Dell support link; dev-only IPC on port 1420; an unknown battery state shows as "Not charging" | open |
| H12 | L | `.github/workflows/windows.yml` | Rust toolchain not pinned; no `cargo audit` | open |

## 4. Where I disagree with Codex (HANDOFF.md)

1. **"Uncommitted docs/permissions":** this is stale. They were committed in `4d93635`, the tree was clean, and `origin/master` exists. Publishing to GitHub (HANDOFF next step #1) has already happened.
2. **"Current-user login setting":** false in the elevated case (H1).
3. **"Multi-battery safety uses the lowest pack":** this was only partly true (C4). The floor had no hysteresis, and the hold could starve the lowest pack.
4. **"Original BIOS state durably recorded before the first change":** the contract re-recorded the "original" on every call (C7). It was not durable.
5. **Dell ranges "cited, not guessed":** they were not enforced anywhere outside tests (C8).
6. **Reminders listed under "What works" in the README:** they were unverified, and failures were silent (H2). They still need a real toast test from an installed build.
7. **"`Win32_Battery` returned no instances":** misleading. A read-only power query shows a battery present (on AC, charging, 39% at review time).
8. **Next-step order:** HANDOFF puts publication first. That's done. The real gate is now Dell discovery, which needs Patrick's authorization to install Dell software. The service should not be built before real attribute and bounds data exist.

Areas of agreement: the default of no capabilities on real hardware, no BIOS passwords in argv, firmware persistence over a universal reset, Tauri/React/Rust, Windows-only scope, and no speculative EC/ACPI access.

## 5. The milestone delivered: M-B "safe core before any writer"

The smallest useful step that needed no hardware and no user decision was to fix every safety and correctness bug that would turn into a real hazard once a Dell writer is wired in, plus the monitor's own real-hardware bugs (H1, H2, H4). No hardware writer was added.

### Changed files (`64c20fe`)
| File | Purpose |
|---|---|
| `crates/chargekeeper-core/src/policy.rs` | C1, C2, C3, C4, C6 and H3; new constants; `Config::validate_for`; 10 tests |
| `crates/chargekeeper-core/src/dell.rs` | C7, C8 and C9; `DOCUMENTED_BOUNDS`, `Bounds::effective`, `thresholds_for`, rollback; `Journal::recorded_original`; 6 tests |
| `crates/chargekeeper-core/src/protocol.rs` | C10: `Request::validate(.., lowest_percent)`; 1 test |
| `crates/chargekeeper-core/src/simulator.rs` | Documented bounds enforced; uses `validate_for`; 2 tests |
| `src-tauri/src/main.rs` | H1 and H2 (`ReminderDelivery`), H4 (mutex), `effective_caps`, `validate_for` in `save_config` and `set_simulation`; 4 tests |
| `src-tauri/src/discovery.rs`, `scripts/discover.ps1` | H6: `wmi_status` |
| `src-tauri/Cargo.toml`, `Cargo.lock` | Replaced `tauri-plugin-{autostart,notification}` with direct `auto-launch 0.6` and `notify-rust 4.18`, both already in the lockfile; the lockfile only lost packages. Added windows-sys features for the mutex |
| `src/main.tsx` | Band minimum is 5 when `set_limit` is available, matching what the backend accepts |

### Commands and results (after the commit's changes, run by Claude)
```
cargo fmt --all --check                                              -> exit 0
cargo clippy --workspace --all-targets --locked --offline -- -D warnings -> exit 0
cargo test --workspace --locked --offline                            -> 42 passed (37 core + 5 host), 0 failed
npm run lint   (tsc --noEmit)                                        -> exit 0
npm run build  (vite)                                                -> exit 0
```
No release build was run, and the GUI was not launched.

### For Codex to review closely
1. **The multi-pack trade-off (C4):** while any pack is below the sailing band, the highest pack may charge above the limit. This favours the floor over longevity.
2. **C2 "keep full" behaviour:** after a top-up, the engine inhibits but doesn't drain while on AC. Decide whether an explicit "discharge to limit after top-up" setting is wanted.
3. **Timeout values:** 4 h per charge phase and 12 h per mode are judgement calls. 12 h might cut short a slow real calibration.
4. **Rollback target:** rollback restores the state read just before the write, not the journaled original. That is intended, but check it.
5. **H2 delivery:** `notify-rust` sees WinRT errors, but Windows can still **silently drop** a toast from an unregistered AppUserModelID. This needs a test from an installed MSI or NSIS build and from a copied exe.
6. **H1 residue:** `auto-launch`'s `is_enabled` and `disable` still check HKLM. A stale HKLM entry from an earlier elevated run reads as enabled, and only an elevated disable removes it.
7. **API changes:** `Journal` has a new required method, and `Request::validate` has a new argument. Neither is used in `src-tauri` yet.
8. Most new core tests were written to fail on the old code, but were **not** run red against it first.

## 6. Proposed work split: no overlapping file edits

| Owner | Files | Next tasks |
|---|---|---|
| **Claude** | `crates/chargekeeper-core/**`, `CLAUDE_REVIEW.md` | C5 after Q5; C12 and C13 storage hardening; the Dell transport contract for real discovered metadata once Q1 data exists; owner identity in `Lease` (C11); review Codex PRs |
| **Codex** | `src-tauri/**`, `src/**`, `scripts/**`, `.github/**`, `README.md`, `report.md`, `HANDOFF.md`, `docs/**` | H5 worker supervision; H7 start hidden in the tray; `reminder_failed` UI; H12 CI pin and `cargo audit`; correcting README and HANDOFF claims per §4; the interactive walkthrough with Patrick |
| **Shared, by PR only** | `Cargo.toml`, `Cargo.lock` | Whoever needs a dependency change raises it in their PR |

The future privileged **service** should be a new crate, such as `crates/chargekeeper-service`, owned by one agent, proposed as Codex, and built only after Q1 produces real Dell metadata. It should consume `chargekeeper-core` without editing it.

### Next milestone (smallest useful step, blocked on Patrick)
**M-C: read-only Dell discovery on the Latitude 7320.** With Patrick's approval, install the official Dell Command | Monitor and/or Configure. Read the charge attribute, its allowed values and ranges, and the password status. **Do not write.** Record the output in `docs/DEVICES.md`. Wire the real bounds into `Bounds` only through `Bounds::effective`, which can only narrow them. Without this data, building the service or the transport would be speculative.

## 7. Open questions for Patrick

1. **Q1:** May an agent install official Dell Command | Monitor and/or Configure, for **read-only** discovery only?
2. **Q2:** Please share the earlier subagents' `.md` files or chat reports (the `/root/ui_review` review and any AGENTS.md) if you have them.
3. **Q3:** Is a BIOS setup or admin password set? Answer yes or no only; don't send the password.
4. **Q4:** Should `64c20fe` and this review be pushed to `origin/master`? I did not push, because pushing publishes and wasn't explicitly requested.
5. **Q5 (C5):** When the battery is hot **and** below 20%, should heat protection win (stop charging) or the safety floor (keep charging)? The current code lets the floor win.
6. **Q6:** For C2, do you want an explicit "drain back to the limit after a top-up" option, or is staying full while on AC fine?
