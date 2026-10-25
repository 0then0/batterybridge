# Security policy

## Supported versions

Security fixes target the latest stable release. Update the GUI, CLI and service
together using the platform installer. Older releases and development snapshots
may not receive separate fixes. Published releases and checksums are listed on
the [Releases page](https://github.com/0then0/batterybridge/releases).

## Report a vulnerability privately

Use GitHub's [private vulnerability reporting](https://github.com/0then0/batterybridge/security/advisories/new).
Do not post exploit details, secrets or personal information in public issues.
If you cannot use that form, open an issue requesting a private security contact
without disclosing the vulnerability itself.

Include the affected version and platform, the component involved, prerequisites,
reproduction steps and the expected impact. A minimal proof of concept or proposed
fix is helpful. Remove device identifiers, credentials and unrelated logs.

Maintainers will assess the report, coordinate a fix and discuss disclosure with
the reporter. This project has no guaranteed response time or paid bug bounty.

## Scope and safe testing

Relevant issues include privilege escalation through the service or installers,
unauthorized policy changes, unsafe IPC parsing and disclosure of private data.
Test only on systems you own or are authorized to assess. Do not run uncontrolled
discharge or firmware writes to demonstrate a security issue.

Ordinary compatibility failures and charging behavior during sleep belong in
[bug reports](https://github.com/0then0/batterybridge/issues/new/choose), unless
they cross a security boundary. See [Security and IPC](docs/security.md) for the
authorization model, diagnostic privacy and hardware safety limits.

Installers are currently unsigned; macOS packages are not notarized. Checksums
detect corruption but do not authenticate the publisher. Download only from this
repository's releases. A passing CI run does not establish hardware compatibility.
