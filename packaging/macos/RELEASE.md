# macOS release packaging

Public release notes live in [CHANGELOG.md](../../CHANGELOG.md). The release
workflow uses the entry matching the version tag for the GitHub Release body.

The universal package contains Intel and Apple Silicon executables, the app,
its GUI login agent and the privileged charging service. Build instructions
are in [the source build guide](../../docs/build.md); installation, updates
and removal are covered in [the installation guide](../../docs/install.md).

Keep the Cargo version, bundle build and Installer receipt consistent with
`packaging/app.py`. Bundle the GUI login agent before signing the app. Published
packages are currently unsigned and not notarized; checksums do not replace signing.
