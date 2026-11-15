# Versioning, compatibility and support policy

UI4ME Core is released as one version number across three artifacts that are
built from the same commit: the `ui4me-core` Python package, the `UI4ME.Core`
PowerShell module and the native host (`ui4me-host`) that both bundle. The
single source of the number is `[workspace.package].version` in `Cargo.toml`
(read by `tools/release/version.py`).

## Semantic Versioning

Versions are `MAJOR.MINOR.PATCH` ([SemVer 2.0](https://semver.org/)). The
**public interface** that the number describes is:

| Surface | What is public |
| --- | --- |
| Python API | Names exported by `ui4me_core` (`__all__`) and documented in [the Python API reference](../reference/python-api.md): classes, methods, parameters, return types, exceptions and documented events. |
| PowerShell API | Commands exported by `UI4ME.Core.psd1` (`FunctionsToExport`) and documented in [the PowerShell command reference](../reference/powershell-api.md): command names, parameters, output objects. |
| Manifest | The `app.toml` schema documented in [the host CLI reference](../reference/host-cli.md#the-apptoml-manifest). |
| Host CLI | `ui4me-host` subcommands and flags documented in the host CLI reference, and its exit statuses. |
| Content | The documented HTML/CSS subset ([HTML and CSS support](../concepts/html-css-scope.md)) and its documented limits (nesting depth, node counts, resource caps). |
| Environment | Documented environment variables (`UI4ME_HOST_BINARY`, `UI4ME_VERBOSE`, `UI4ME_TEST_MODE`, ...). |

Not public: anything with a leading underscore, undocumented modules and
commands, the IPC protocol wire format (see below), log text, the layout of
files inside the wheel or module, and rendering results beyond the documented
subset (pixel-exact output can change in any release).

* **PATCH** (1.0.x): bug fixes, security fixes, documentation, performance, and
  stricter handling of input that was already outside the documented limits.
  No public interface is removed or changed incompatibly. A patch may make a
  previously crashing or hanging input fail cleanly with a documented error.
* **MINOR** (1.x.0): new features and new public names, new manifest keys
  (optional, with defaults), new protocol capabilities, deprecations, and
  dropping a platform or interpreter that reached end of life (see support).
* **MAJOR** (x.0.0): removals of deprecated interfaces and any other
  incompatible change.

### Deprecation

An interface is deprecated in a minor release: the documentation says so, the
changelog lists it, and the Python API emits `DeprecationWarning` (PowerShell:
a `Write-Warning` once per session) where practical. A deprecated interface
keeps working for **at least one further minor release and is removed no
earlier than the next major release**.

## Protocol and capability policy

The host and the bindings talk over a private, versioned IPC protocol
([protocol reference](../reference/protocol.md), currently version 1). The
supported programming interfaces are the bindings; the protocol is documented
for binding maintainers.

* The bindings shipped in a release always match its host. Mixing a binding
  and a host from different releases is supported only within the same
  protocol version.
* Within a protocol version, changes are **additive**: new methods, new
  notification types, new optional fields and new `host.capabilities` feature
  keys. A binding must ignore unknown notification types and fields, and must
  detect optional behaviour through `host.capabilities` (`features.*`), never
  through the version number.
* Removing or changing the meaning of a method, field or notification requires
  a new protocol version. The `host.hello` handshake rejects an unsupported
  version with an explicit error instead of misbehaving; such a change is only
  released in a new major version.
* `capability_schema` changes follow the same rule: additive within a schema,
  a new schema number for incompatible changes.

## Support policy

* **Supported releases:** the latest patch of the latest minor series. Fixes,
  including security fixes ([SECURITY.md](../../SECURITY.md)), are released as
  a new patch of that series; earlier series are not patched.
* **Python:** CPython versions that are not end-of-life when a minor release is
  made, from the `requires-python` floor up to the newest stable CPython. Every
  claimed version is tested in CI (`.github/workflows/ci.yml`, job `python`). A
  version that reaches end of life is dropped only in a minor release, never
  in a patch. (Python 3.10 reached end of life in October 2026; 1.0.x keeps
  supporting it, and 1.1 may drop it.)
* **PowerShell:** PowerShell 7.2+ on Linux and Windows, and Windows PowerShell
  5.1 on Windows, all tested against the real host in CI.
* **Platforms:** Linux x86-64 with glibc 2.34+ in a Wayland session, and Windows
  x64. The qualified desktops of each release are listed in its release notes;
  other configurations are expected to work but are not claimed as tested
  ([platforms](../guides/platforms.md)).
* **Development status:** the PyPI classifier is `Development Status :: 4 - Beta`
  until the stable-release gates of the engineering audit
  (`docs/audit/05-release-readiness.md`: R1-R10, P1-P7, P12, P13) pass. Moving back to
  `5 - Production/Stable` is a release decision recorded in the changelog.

## Yanking and rollback

Registry versions are immutable: a published version number is never reused
or overwritten. If a release is broken (it fails to install, crashes on
supported platforms, or has a security problem):

1. **Fix forward.** Prepare the next patch version with the fix and publish it
   through the normal gated process ([publishing](publishing.md)).
2. **PyPI:** yank the broken version (project owner: *Manage project -> Releases
   -> Options -> Yank*, with a reason that names the replacement). Yanked
   versions stay installable when pinned exactly (`==`), so existing
   lock files keep working, but resolvers no longer pick them.
3. **PowerShell Gallery:** unlist the broken version (owner: package page ->
   *Manage* -> *Unlist*). `Install-Module -RequiredVersion` still finds it;
   searches and unversioned installs do not.
4. **GitHub release:** keep the release and its assets (they are the
   provenance record); edit the notes to start with a warning that names the
   replacement version, and remove the *Latest* flag if the replacement is not
   out yet.
5. **Record it:** add a "Yanked" line under the version in `CHANGELOG.md`
   (`## [1.0.3] - 2026-12-01 [YANKED]`) with the reason, and open or update the
   issue that tracks the defect.

A module-only or packaging-only problem in the PowerShell Gallery also needs a
new patch version and tag: the Gallery module is always built from the commit
of its `v<version>` tag (publish-powershell.yaml enforces this).

## Changing the version

`python tools/release/bump_version.py X.Y.Z` rewrites the format-forced
locations (Cargo, Cargo.lock, pyproject, the Python package and client, the
PowerShell manifest and module, the Windows host manifest);
`python tools/release/version.py --check --strict` verifies them, and
`python tools/release/bump_version.py --list` shows every remaining literal of the
current version. Workflows and release verifiers read `version.py` and never
need editing for a release.
