# Developing a backend

[Documentation index](README.md)

A backend adds device-specific discovery and charging operations while keeping the shared policy independent of vendor details. Read the [architecture guide](architecture.md) before choosing an integration point.

## Discovery

`Platform::discover` collects device metadata and known interfaces into `Facts`. The pure `resolve` function applies OS, architecture, vendor and interface checks to produce a `ProbeReport`. Each candidate explains why it was selected or rejected. Tests use the same resolver with synthetic facts.

There is currently one controller candidate per operating system. When adding competing adapters, list all relevant candidates and give exact interface matches precedence over broad vendor matches. Linux's generic threshold backend is vendor-independent. Windows Lenovo selection also requires compatible methods and capability records.

Monitoring remains independent of controller activation. A missing or inaccessible controller should still allow battery status and a compatibility report wherever monitoring is available.

## Traits and code layout

- `BatteryMonitor` reads battery status.
- `ChargeController` exposes capabilities, applies thresholds and provides optional gate/discharge operations. `release(scope)` returns hardware to the user with an explicit scope: `Overrides` for daemon-held state only, `Everything` for native thresholds as well. Implement both deliberately and try every step.
- `DeviceInfoProvider` supplies OS, vendor and model metadata.
- `PowerMonitor` exposes AC connectivity.

DTOs belong in `batterybridge-protocol`. Policy and shared traits belong in `batterybridge-core`, which forbids unsafe code. Platform adapters live in `batterybridge-platform`; a separate backend crate is appropriate when its dependencies or size justify it.

Keep model names, native paths, SMC keys and WMI identifiers inside their adapters. Core policy should operate on capabilities and battery status rather than vendor conditionals.

## Declare capabilities

Use `Supported` for a recognized, validated interface contract. Use `Experimental` for an implemented private interface without recorded device-level write validation. Use `Unsupported` when a required feature is absent and `Unknown` when discovery cannot establish availability.

Provide a reason for every feature. Fixed charge modes belong in `allowed_limits`; requests outside that list must be rejected rather than rounded silently. Document whether a percentage is enforced by a native threshold or a service-controlled gate.

Probe reads only. It must not test permissions by writing hardware. Check write access when applying a request and return a typed error if access is denied.

## Diagnostics and fixtures

Collect named fields and known interfaces. Use local battery ordinals such as `BAT0` in reports. Keep serials, raw WMI paths and provider objects out of public DTOs and test fixtures.

Add JSON under `tests/fixtures` and register it in `crates/platform/tests/fixtures.rs`. Include expected backend selection, all six capability states and battery status. Mark synthetic input explicitly. Cover rejected devices as well as matching ones, including a matching vendor with missing interfaces.

Test the actual adapter with temporary files or a mock transport in addition to testing the resolver. Fixtures establish deterministic software behavior; [device validation](hardware.md#device-validation) establishes charging behavior on a physical laptop.

## Writes and recovery

The service owns controller mutations. Expose fixed operations through the protocol and validate both request ranges and backend-specific values. Use reliable readback where available; document unavailable or ambiguous readback.

Keep start strictly below stop during each intermediate write. Check return codes and recover from partial updates. Release force discharge on target completion, sensor failure, errors, shutdown and suspend. Classify each mechanism as a native threshold (kept on orderly stop, persistence unknown until measured), a daemon-held gate or an override (always released on stop), document it in the [lifecycle guide](lifecycle.md), and combine step results with `merge_steps` so one failure never skips an independent step. Test these transitions with a fake controller or transport before supervised device testing.

Native writes are isolated in the platform modules: Linux sysfs operations in `linux.rs`, macOS FFI in the IOKit/SMC/power modules, and Windows Win32 power calls in `windows/power.rs`. Each unsafe call needs a local safety explanation covering ABI layout, ownership, pointer validity and resource lifetime.

## Protocol changes

Version the wire protocol independently of configuration and report formats. Requests reject unknown fields. Update Rust DTOs, GUI types, callers, tests and relevant documentation together. The CLI and GUI should remain clients of the same service contract.

Before promoting an experimental interface to verified device support, record the model, firmware/driver versions, observed charging transitions and restore procedure in a sanitized validation report.
