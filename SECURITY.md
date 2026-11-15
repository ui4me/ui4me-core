# Security policy

## Supported versions

Security fixes are released for the **latest published patch release of the
latest minor series** of `ui4me-core` (PyPI) and `UI4ME.Core` (PowerShell
Gallery). Both channels bundle the same native host and are fixed together.
Older versions are not patched: upgrade to the latest patch release. The
support policy is described in [docs/release/versioning.md](docs/release/versioning.md).

| Version | Supported |
| --- | --- |
| 1.0.x (latest patch) | yes |
| < 1.0 | no |

## Reporting a vulnerability

Please **do not open a public issue** for a suspected vulnerability.

Report it privately through GitHub: on the repository's **Security** tab,
choose **Report a vulnerability** (private vulnerability reporting). If that
option is not available to you, open an issue that says only that you have a
security report and need a private contact; do not include details.

Please include the affected version and platform, a description of the
impact, and steps or a minimal script to reproduce it. We aim to acknowledge
a report within 7 days and to agree on a disclosure timeline with you. Fixes
are released as a new patch version on both registries; the advisory credits
the reporter unless you ask otherwise.

## Scope

In scope:

* the native host (`ui4me-host`): its local IPC endpoint and token
  authentication, process supervision, and the handling of documents, styles,
  fonts and images it renders (for example crashes, memory exhaustion or
  hangs caused by content, which the host is designed to bound or reject);
* the Python and PowerShell bindings and the published packages themselves
  (for example a wheel or module that could execute something it should not).

Out of scope: vulnerabilities in your own application logic, in content you
choose to load with full trust, or in the operating system, desktop session
or Python/PowerShell runtime. UI4ME runs no JavaScript and opens no network
listener; the IPC endpoint is a per-user local socket or named pipe.

## Verifying what you install

Each GitHub release lists the SHA-256 of every file (`SHA256SUMS`, and
`SHA256SUMS-powershell` for the module package). PyPI files carry PEP 740
publish attestations, and release files carry GitHub build-provenance
attestations (`gh attestation verify <file> --repo ui4me/ui4me-core`) when the
repository is public. Host SBOMs (CycloneDX) are attached to each release.
