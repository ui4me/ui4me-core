# UI4ME Core 1.0.0

First stable release of the **native HTML/CSS desktop runtime** for Python.

- Linux x86-64 (manylinux 2.34+, glibc 2.34+) and Windows x86-64 wheel distributions.
- Rust-owned desktop windows and application lifecycle; Python logic in a separate, supervised process.
- Local HTML, CSS, system fonts, common forms, CSS variables, responsive layout, events, accessibility and native dialogs.
- Custom title bars, drag/resize regions, and bounded live RGBA bitmap surfaces.
- No bundled Chromium, WebKit or JavaScript engine; this is a documented desktop HTML/CSS subset, **not** a complete web browser.
- Python 3.10+; wheel installation does not require Rust.
- `ui4me-new` project generator (HTML, CSS, Python and a manifest that names your virtual environment).
- Bundled `ui4me-python` coding-agent skill with `ui4me-skill`, installing into per-user Claude Code, Codex, OpenCode, GitHub Copilot, Cursor and Gemini CLI skill folders.

Documentation: [Getting started](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/quickstart.md) · [Python API](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/python-api.md) · [Platforms and limitations](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md).

**Platforms qualified:** Debian 13/KDE Plasma native Wayland and Windows Server 2022 Desktop Experience (RDP). Other configurations are not claimed as tested. Refer to `docs/STATUS.md` for exact evidence.

This release was intentionally published through a manual GitHub Actions workflow and PyPI Trusted Publishing.
