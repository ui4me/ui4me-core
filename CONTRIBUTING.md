# Contributing to UI4ME Core

UI4ME is a native HTML/CSS desktop runtime. Changes to the native host, protocol or rendering contracts should have a real application justification.

Before proposing changes, read [development guide](docs/guides/development.md), [platform support](docs/guides/platforms.md) and [building and verifying releases](docs/release/building.md). Keep fixes focused and add regression tests. Use targeted Rust tests during development, then run the full verifier at integration milestones. Never change or delete a test just to make a gate green. Tests requiring native KDE/Windows must be validated on the actual desktop, not merely source-compiled.

Vendor patches under `third_party/` and `patches/` have ownership, license and provenance obligations. Prefer upstream fixes where practical, and preserve license texts. Do not store credentials, build output, local verification logs, user settings or VM details in the repository.

Releases are owner-controlled through the [manual publishing workflow](docs/release/publishing.md). Pull requests and ordinary pushes do not publish.
