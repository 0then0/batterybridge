# Contributing to BatteryBridge

Contributions are welcome for device support, charging-policy correctness, diagnostics, accessibility and documentation. Start with the [architecture guide](docs/architecture.md) to find the relevant component.

Participation follows the [Code of Conduct](CODE_OF_CONDUCT.md). Report vulnerabilities
privately using the [security policy](SECURITY.md), rather than a public issue.

## Report a problem

Search existing [issues](https://github.com/0then0/batterybridge/issues) before using the [bug or feature forms](https://github.com/0then0/batterybridge/issues/new/choose). Include the BatteryBridge version, OS version, expected behavior, actual behavior and reproduction steps. For compatibility problems, attach a reviewed `batterybridge probe --json` report and describe the affected charging feature.

Remove personal identifiers from logs and reports. Include device model and relevant firmware or driver versions, but omit serials, UUIDs and raw WMI instance paths. CI results and synthetic fixtures should be identified separately from tests on a physical laptop.

## Development setup

Follow [building from source](docs/build.md). Use the committed Cargo lockfile. CLI and service require Rust 1.90+; the native desktop app requires Rust 1.98+ and the graphics prerequisites in the build guide.

## Local checks

From the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
```

Use `cargo fmt --all` to format Rust. GUI tests check the seven translation catalogs, font coverage, layout widths, draft settings and policy warnings. Run packaging checks with `python3 -m unittest discover -s packaging/tests -v`; on macOS, also run `python3 -m unittest discover -s packaging/macos/tests -v`. Inspect changes before committing. Git attributes keep text files in LF format on all platforms.

For CLI/service work without desktop prerequisites, check the default workspace members:

```sh
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --locked
```

Tests use synthetic facts, fake controllers and temporary files. Normal tests do not write battery controls or install services. Windows CI additionally checks installer ACL behavior and native preparation in its disposable VM. The release workflow tests installation and removal only on disposable CI machines.

## Add a backend

Read [developing a backend](docs/contributing-backends.md). Include discovery reasons, per-feature capabilities, typed errors and tests for both matching and rejected devices. Hardware validation belongs in a separate report with the tested model, firmware and restore procedure.

## Hardware testing

The [hardware guide](docs/hardware.md#device-validation) describes supervised validation. The automated hardware test only reads device information and is ignored by default. Run writes only on a device whose owner has agreed to the test, and supervise force discharge throughout.

## Submit a pull request

Explain the problem, resulting behavior and checks you ran. Keep unrelated changes separate and add regression coverage for changed behavior. Update user documentation when commands, capabilities or installation requirements change.

Use the pull request template to describe validation and compatibility. Add
user-visible changes to the unreleased section of [CHANGELOG.md](CHANGELOG.md),
or create that section above the latest release if it is absent. Changes to
dependencies must include the lockfile; avoid unrelated dependency updates.

Use Conventional Commits in English: `feat`, `fix`, `refactor`, `chore`, `docs` or `test`. The same format is suitable for pull request titles.
