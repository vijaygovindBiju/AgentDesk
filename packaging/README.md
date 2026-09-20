# AgentDesk release packaging

This directory documents the release contract consumed by `../install.sh`.

## Linux daemon archive

The workspace version in `core/Cargo.toml` is the release version source of
truth. A release tag must be the same version with a `v` prefix, for example
`core/Cargo.toml` version `0.1.0` is published as tag `v0.1.0`. The release
workflow publishes these assets under that tag:

```text
AgentDesk-vX.Y.Z-linux-x86_64.tar.gz
AgentDesk-vX.Y.Z-linux-aarch64.tar.gz
SHA256SUMS
```

Each Linux archive must contain one executable at its root:

```text
agentdesk
```

The archive should contain no other files, directories, symlinks, or nested
paths. The installer rejects archives that do not contain exactly this single
root-level executable.

The executable is the `agentdesk-server` binary built from `core/agentdesk-server`.
The archive must be built for the target Linux architecture and must not require
Rust, Cargo, Flutter, Android SDK tools, ADB, or source files on the user's
machine.

`SHA256SUMS` must contain a line for every published artifact, including the
Linux archives and Android APK. The installer downloads the checksum file over
HTTPS and refuses to install when the selected archive is missing or its SHA-256
does not match.

## Android application

The direct-download Android artifact produced by the release workflow is:

```text
AgentDesk-vX.Y.Z.apk
```

It is built from `mobile/` for Android ARM64 and distributed separately from the
Linux installer. The workflow currently uses Flutter's default release signing
configuration; configure repository signing secrets before treating it as a
production-signed APK.

## Release hosting

The installer defaults to the repository's current GitHub remote:

```text
https://github.com/vijaygovindBiju/AgentDesk
```

It resolves `VERSION=latest` through the GitHub Releases API and downloads
assets from the matching release tag. A release process must publish the
artifacts above before the installer can succeed. The installer also supports
`AGENTDESK_REPOSITORY` and `AGENTDESK_RELEASE_BASE_URL` for an explicitly
configured mirror; the base URL must use HTTPS.

## Trust model

- The installer downloads only from HTTPS URLs.
- The archive is never executed before extraction and SHA-256 verification.
- The checksum must be present in `SHA256SUMS`; missing or invalid entries fail
  the installation.
- The installer runs as the invoking user and installs a systemd **user**
  service. It refuses to run as root.
- Release signing is not yet configured in this repository. SHA-256 protects
  against transfer corruption and accidental artifact mismatch; production
  releases should add signed release metadata or artifact signatures before
  treating the distribution channel as fully hardened.
