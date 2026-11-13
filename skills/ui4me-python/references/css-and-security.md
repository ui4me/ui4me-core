# UI4ME rendering, security, and platform boundaries

UI4ME renders a practical **desktop subset** of HTML/CSS without a browser. Common selectors, CSS custom properties, font faces, flex/grid/tables, overflow/scrolling, forms, and native text input/IME are supported. Static SVG and local image resources work; complex browser/SVG features are not automatically supported.

- **No JavaScript execution:** no DOM JS methods, `fetch`, browser navigation, service workers or browser-based IPC. Application behavior is implemented in Python via typed document commands and events.
- **Local resources only:** ordinary `href="style.css"`, `src="image.png"` and `@font-face` URLs resolve relative to the HTML/asset root; external network URLs are not a runtime dependency.
- **Trusted backend, not web sandbox:** the backend is application-owned Python and has its normal OS privileges. Do not render untrusted HTML as though UI4ME were a hardened browser sandbox.
- **Use native dialogs and clipboard**; do not expect HTML file input to perform remote uploads or a JavaScript File API.
- **Stale handles:** a new document replaces the old document generation. Do not reuse old node IDs after reload.
- **Bounds:** IPC, resource bundles, image buffers and event queues are bounded. A live bitmap must use raw RGBA8 frames of the correct size, reused mapped slots and explicit cleanup.
- **Platform differences:** some native window positioning/focus/visibility requests cannot be implemented by the current Wayland backend. Check `app.capabilities()` and handle explicit unsupported errors. Windows has separate native limitations documented in `docs/guides/platforms.md`.

Use `ui4me-host doctor` for host/environment status. For the supported profile and technical contracts refer to repository `docs/CORE_1.0_SPEC.md`, `docs/PROTOCOL_DOCUMENTS.md` and `docs/CAPABILITY_SCOPE.md`.
