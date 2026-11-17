# Contributing

Use Windows for this release. Run `npm ci`, `npm run lint`, `npm run build`,
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`
and `cargo test --workspace --locked`. CI uses the same commands.

Keep policy changes in the platform-independent core. Add a test for meaningful
safety or state transitions; use mocked transports rather than BIOS writes in CI.
Do not add vendor writes without an official source, runtime bounds, secure
transport, original-state journaling, readback and real-device evidence.

Keep changes focused. Use native controls and the existing theme. Do not add
network access, accounts, telemetry or a privileged generic command runner.
Never put credentials, service tags, serials or raw device dumps in issues/logs.

Open a small pull request explaining the behavior, test evidence and device/BIOS
coverage. Experimental does not mean verified. See [provider guide](docs/PROVIDERS.md).

For security issues, follow [SECURITY.md](SECURITY.md) before posting details.
