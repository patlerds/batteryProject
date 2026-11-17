# Adding a provider

1. Research official vendor documentation and cite the exact interface/version
   in PLAN.md. Avoid guessed WMI classes and raw register access.
2. Implement `BatteryProvider` and `ChargeController` from the Rust core. Publish
   only capabilities demonstrated by runtime probes on the actual device.
3. Keep privileged writes in the future authenticated service. The UI gets typed
   operations, never executable paths, command strings or attribute names.
4. Discover allowed values, min/max and gap at runtime. Fail closed on unknown
   metadata, BIOS drift or credentials. A model name alone is insufficient.
5. Persist original state before changing it; confirm writes through a separate
   read. Keep a pending top-up journal and conservatively restore on restart.
6. Supply mock transport tests for parsing, command construction, invalid input,
   access denial, wrong password, partial failure, readback mismatch and rollback.
7. Complete the per-device checklist. Add evidence to DEVICES.md, marking
   documented, experimental or physically verified accurately.

The current Dell module is a pure command builder/parser and transaction contract.
It is not a privileged executable adapter. The lease/protocol module is a tested
future-service building block, not an installed watchdog.
