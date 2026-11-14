# HTML/CSS scope, security and platform boundaries

UI4ME renders a practical desktop profile of HTML and CSS natively. It is not a browser engine; design with ordinary, well-supported HTML/CSS.

## Works well

- Semantic HTML: headings, paragraphs, lists, tables, `<form>`, `<label>`, `<input>` (text, number, checkbox, radio, file, ...), `<textarea>`, `<select>`, `<button>`, `<progress>`, `<dialog>`, the `popover` attribute, images (local raster and static SVG).
- CSS selectors, cascade, custom properties (`var()`), `@media` including `prefers-color-scheme` and `prefers-reduced-motion`, `@font-face` with local fonts, `calc()`, `light-dark()`, `@container`, `@layer`, box model, flexbox, grid, tables, multi-column, positioning, overflow/scrolling, borders, backgrounds and a selected set of visual effects, transitions and `@keyframes` animations. Not every browser property is recognized or painted identically.
- Native text editing, selection, IME and accessibility for controls.
- Live pixel output with `<img src="ui4me-bitmap://id">`; in-memory files with `ui4me-res://bundle/path`.

When a property seems ignored: `document.computed_style(["prop"], "id")` reports `supported`/`unsupported`/`invalid`, and `document.diagnostics()` lists CSS and resource problems. Prefer simple, standard layouts (flex/grid) over browser-specific tricks.

## Not available

- JavaScript in any form: browser DOM/Web APIs, `fetch`, WebSockets, storage APIs, service workers. A page containing `<script>`, `<iframe>`, `<object>`, `<embed>`, `on…=` attributes, `srcdoc`, `contenteditable`, `<input type="color">`/date/time inputs, or duplicate `id`s **fails to load** with `RpcError` -32602.
- Network loading: `http(s)://` images, stylesheets, fonts. Bundle assets locally.
- `<canvas>` drawing, audio/video playback (both are inert empty boxes; use a live bitmap for pixels), WebGL, printing, Shadow DOM.
- Navigation: `<a href>` never navigates; it emits `link_activated` (`href`, `url`). Switch screens with `window.load_file`/`load_html`; open web links with `app.open_external(url)`.
- Rich text editing and HTML/image clipboard (plain text only); colour/date pickers; `<datalist>` suggestions.

## Assets

Relative URLs resolve against the HTML file's folder, or `asset_root` for `open_html`/`load_html`; keep every asset inside that root. An HTML string without `asset_root` has no base folder. `data:` URLs (up to 4 MiB) and `ui4me-res://` bundles also work; every other scheme, including `http(s):`, is refused. Raster formats: PNG, JPEG, GIF/WebP (first frame), BMP. Per-load HTML is limited to 1 MiB; text per update to 16 KiB.

## Security model

- The Python backend is trusted application code with the user's normal OS permissions. UI4ME is **not** a sandbox for untrusted web content; do not load HTML from untrusted sources.
- Treat `insert_html`/`replace_html`/batch `html` as code: escape user data with `html.escape()`, or use `set_text`/`set_value`.
- The IPC endpoint is private and authenticated with a per-launch token. `Application.start()` removes `UI4ME_ENDPOINT`/`UI4ME_TOKEN` from `os.environ` in the backend; never print, log or persist them.
- `app.open_external(target)` hands a path or URL to the desktop's default handler; validate targets that come from user input.
- File paths from dialogs, file inputs and drops are chosen by the user; the backend reads them directly (no data is copied over IPC).

## Platform differences

| Topic | Linux (Wayland) | Windows |
| --- | --- | --- |
| Window position (`x`, `y`) | Not supported (`RpcError` -32001 / `observed["position"] is None`) | Supported |
| Focus / raise | Compositor decides; requests may be ignored | Supported within OS focus rules |
| File dialogs | Desktop portal; its own look and parenting | Native common dialogs |
| Desktop preferences | Depend on the desktop; values may be `None` | Reported by Windows; still handle `None` |

Check `app.capabilities()` and `window.state()["observed"]`, and handle `RpcError` instead of assuming a request took effect. Linux x86-64 with glibc 2.34+ and a Wayland session, and Windows x86-64, are supported; macOS, X11-only and ARM64 are not.

More: https://github.com/ui4me/ui4me-core/blob/main/docs/concepts/html-css-scope.md and https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md
