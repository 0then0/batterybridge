# Repository and releases

[Documentation index](README.md)

## Find project resources

- [Issues](https://github.com/0then0/BatteryBridge/issues): bugs, device compatibility and feature requests.
- [Pull requests](https://github.com/0then0/BatteryBridge/pulls): proposed changes and code review.
- [CI](https://github.com/0then0/BatteryBridge/actions/workflows/ci.yml): build and test results for Linux, Windows and macOS.
- [Release artifacts](https://github.com/0then0/BatteryBridge/actions/workflows/release.yml): packaged platform builds.
- [Releases](https://github.com/0then0/BatteryBridge/releases): published releases, when available.

For a compatibility issue, include the affected feature, device model, relevant firmware/driver versions and a reviewed `batterybridge probe --json` report. See [Contributing](../CONTRIBUTING.md) for reporting and pull request guidance.

## Continuous integration

The CI workflow runs on pushes and pull requests. It installs native graphics prerequisites, then runs Rust formatting, Clippy, tests and builds on Linux, Windows, Apple Silicon macOS and Intel macOS. GUI tests exercise catalogs, fonts and layouts without hardware access. Windows also checks installer ACLs, task lifecycle and the actual NSIS preparation hook. Linux starts a synthetic preview under Xvfb.

A successful run confirms software checks on the runner operating systems. Device-level charging behavior is validated separately using the [hardware procedure](hardware.md#device-validation).

## Build artifacts

The release workflow runs for `v*` tags or through manual workflow dispatch. It produces archives for Linux x86_64, Windows x86_64 and macOS universal (Intel and Apple Silicon), containing:

- CLI and service executables.
- Native desktop bundles under `gui`.
- Service integration files under `packaging`.
- Documentation, license and icon assets.
- `SHA256SUMS.txt` with hashes for the staged files.

Download an artifact from the completed workflow run and extract its archive. GitHub may require sign-in to download workflow artifacts. Desktop bundles are unsigned unless signing is configured separately. The `installers-*` artifacts contain a universal macOS `.pkg`, Windows setup `.exe`, and Linux `.deb`/`.rpm` packages. Each includes its background component. Developer archives still require separate service installation.

## Prepare a release

Maintainers should verify the selected commit's CI results, review the current version entry in [CHANGELOG.md](../CHANGELOG.md) and compatibility information before creating a version tag. Pushing a `v*` tag builds all three platforms and publishes a GitHub Release only after every build succeeds. Tags containing a hyphen, such as `v0.1.0-dev.1`, create pre-releases. Manual workflow dispatch builds artifacts without publishing.

Application version comes from the root `Cargo.toml`; numeric macOS build and receipt metadata come from `packaging/app.py`. Release 0.3.1 uses bundle build 6 and receipt 0.3.1.6. Version 0.2.0 was the last release of the previous GUI. Keep the version, release tag and Cargo lockfile metadata consistent. Update the README download links, installation guide and changelog before tagging. CI must pass for the selected commit. The release workflow rejects a tag that differs from the Cargo version and takes the release body from its dated changelog entry. The workflow generates an outer `SHA256SUMS.txt` covering downloadable packages and archives.

Keep CLI, GUI and service versions together when distributing a build. Protocol-incompatible components reject requests explicitly.

## Dependency updates

Dependabot checks Cargo and GitHub Actions dependencies monthly. Patch and minor updates are grouped, major updates are excluded from automatic proposals, and automatic merging is disabled. Review dependency changes through the same checks as other pull requests.
