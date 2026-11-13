# UI4ME Core documentation

Build native Python-driven desktop applications from local HTML/CSS, without a browser runtime or JavaScript. Choose a starting point:

| Audience | Read first | Then |
| --- | --- | --- |
| **New developer** | [5-minute quickstart](guides/quickstart.md) | [Python API reference](reference/python-api.md), [recipes](guides/patterns.md) |
| **Coding agent** | [Install Agent Skill](guides/agent-skills.md) | [Skill source](../skills/ui4me-python/SKILL.md) |
| **Desktop integrator** | [Platform behavior](guides/platforms.md) | [Architecture](CORE_1.0_SPEC.md), [wire protocol](PROTOCOL.md) |
| **Maintainer** | [Building and testing](guides/development.md) | [Capability scope](CAPABILITY_SCOPE.md), [release policy](release/PUBLISHING.md) |

## API and conceptual references

- [Python API](reference/python-api.md) — application/window/document handles, callbacks, forms, resources, native services.
- [Patterns](guides/patterns.md) — generated HTML, updates, styles, native chrome, live bitmaps.
- [Platform support](guides/platforms.md) — qualified environments, packaging constraints, native limitations.
- [Technical Core contract](CORE_1.0_PROTOCOL.md) and [document commands](PROTOCOL_DOCUMENTS.md).
- [Supported feature decisions](CAPABILITY_SCOPE.md) and [release status](STATUS.md).

## Release engineering

- [Manually publish to PyPI/GitHub](release/PUBLISHING.md)
- [Linux builds](release/BUILDING_LINUX.md) / [Windows builds](release/BUILDING_WINDOWS.md)
- [Release notes](release/RELEASE_NOTES_1.0.0.md)

## Historical engineering records

The `docs/verification/`, `BASELINE_*.md`, `SPEC.md`, and older release notes are retained as **historical qualification evidence**, not beginner setup instructions. Preserve the evidence chain rather than discarding data used to qualify 1.0.0.
