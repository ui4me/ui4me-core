#!/usr/bin/env python3
"""Regenerates the resources-svg fixtures (007 T045).

  svg-images/             HTML fixture for the oracle's pixel/geometry capture (OC task T047): file SVGs with and
                          without width/height, viewBox only, <text>, currentColor, a relative image href, a
                          nested data: SVG at depth 5, an SVGZ stub, Bootstrap-style data: glyphs and an
                          inline <svg>; variants at scales 1.0, 1.25, 1.5 and 2.0.
  big-svg.py              generator of an SVG with more than 1,000,000 elements (written to stdout; the engine
                          test builds the same document in memory, nothing large is stored)
  corpus-manifest.json    SHA-256 of the generated files
The engine side (usvg/resvg adapter, T046/T048-T052) is blocked: those crates are not in the dependency
lock (specs/007-resources/ERRATA.md E-007-002).
Run:  python3 -I generate.py   (from this directory)
"""
import base64, gzip, hashlib, json, os

HERE = os.path.dirname(os.path.abspath(__file__))
D = os.path.join(HERE, "svg-images")
files = {}


def put(rel, text):
    data = text if isinstance(text, bytes) else text.encode()
    p = os.path.join(D, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "wb").write(data)
    files[rel] = data


NS = 'xmlns="http://www.w3.org/2000/svg"'
put("svg/sized-40x20.svg", f'<svg {NS} width="40" height="20" viewBox="0 0 40 20"><rect width="40" height="20" fill="#2a7"/><circle cx="20" cy="10" r="8" fill="#fff"/></svg>\n')
put("svg/viewbox-only.svg", f'<svg {NS} viewBox="0 0 30 15"><rect width="30" height="15" fill="#47c"/><path d="M0 0 L30 15" stroke="#fff" stroke-width="2"/></svg>\n')
put("svg/no-size-no-viewbox.svg", f'<svg {NS}><rect width="24" height="24" fill="#c47"/></svg>\n')
put("svg/with-text.svg", f'<svg {NS} width="60" height="24"><rect width="60" height="24" fill="#eee"/><text x="4" y="16" font-size="12">Hi</text></svg>\n')
put("svg/current-color.svg", f'<svg {NS} width="24" height="24" viewBox="0 0 24 24"><rect width="24" height="24" fill="currentColor"/></svg>\n')
put("img/dot.png", base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="))
put("svg/relative-image-href.svg", f'<svg {NS} xmlns:xlink="http://www.w3.org/1999/xlink" width="20" height="20"><image xlink:href="../img/dot.png" width="20" height="20"/></svg>\n')
engine = {}  # engine-only data: 002's fixture rules (FR-002) allow only relative local references in fixtures
engine["file-href.svg"] = {"text": f'<svg {NS} width="20" height="20"><image href="file:///etc/hostname" width="20" height="20"/></svg>\n'}

# nested data: SVG, depth 5 (the engine allows 4)
inner = f'<svg {NS} width="10" height="10"><rect width="10" height="10" fill="#c80"/></svg>'
for _ in range(4):
    inner = f'<svg {NS} width="10" height="10"><image href="data:image/svg+xml;base64,{base64.b64encode(inner.encode()).decode()}" width="10" height="10"/></svg>'
engine["nested-depth5.svg"] = {"text": inner + "\n"}
engine["sample.svgz"] = {"hex": gzip.compress(f'<svg {NS} width="8" height="8"/>'.encode(), mtime=0).hex()}

checkbox = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20'><path fill='none' stroke='%23fff' stroke-linecap='round' stroke-linejoin='round' stroke-width='3' d='m6 10 3 3 6-6'/></svg>"
select = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'><path fill='none' stroke='%23343a40' stroke-linecap='round' stroke-linejoin='round' stroke-width='2' d='m2 5 6 6 6-6'/></svg>"
put("svg/checkbox.svg", checkbox.replace("%23", "#").replace("'", '"') + "\n")
put("svg/select-caret.svg", select.replace("%23", "#").replace("'", '"') + "\n")
engine["data-url-checkbox"] = {"text": "data:image/svg+xml," + checkbox}
engine["data-url-select"] = {"text": "data:image/svg+xml," + select}
html = f"""<!doctype html>
<meta charset="utf-8">
<title>resources-svg</title>
<style>
body {{ margin: 8px; font: 12px sans-serif; color: #b33 }}
.row {{ display: flex; gap: 8px; align-items: flex-start; margin: 4px 0 }}
.glyph {{ width: 20px; height: 20px; background: #0d6efd center/ 100% no-repeat url("svg/checkbox.svg") }}
.caret {{ width: 32px; height: 20px; border: 1px solid #aaa; background: #fff right 4px center/16px 12px no-repeat url("svg/select-caret.svg") }}
</style>
<div class="row">
<img id="sized" data-probe="sized" src="svg/sized-40x20.svg">
<img id="viewbox" data-probe="viewbox" src="svg/viewbox-only.svg">
<img id="nosize" data-probe="nosize" src="svg/no-size-no-viewbox.svg">
<img id="text" data-probe="text" src="svg/with-text.svg">
<img id="currentcolor" data-probe="currentcolor" src="svg/current-color.svg">
</div>
<div class="row">
<img id="relhref" data-probe="relhref" src="svg/relative-image-href.svg">
</div>
<div class="row">
<div id="checkbox" class="glyph" data-probe="checkbox"></div>
<div id="select" class="caret" data-probe="select"></div>
<svg id="inline" data-probe="inline" width="30" height="30" viewBox="0 0 30 30"><circle cx="15" cy="15" r="12" fill="#a3c"/></svg>
</div>
"""
put("index.html", html)

probes = [{"probe_id": i, "observables": ["geometry", "pixels"], "geometry_class": "G1"} for i in
          ["sized", "viewbox", "nosize", "text", "currentcolor", "relhref", "checkbox", "select", "inline"]]
variant = lambda vid, dpr: {"variant_id": vid, "viewport": {"width": 360, "height": 240}, "dpr": dpr, "color_scheme": "light", "reduced_motion": "no-preference"}
sources = [{"path": k, "sha256": hashlib.sha256(v).hexdigest(), "role": "html" if k.endswith(".html") else ("svg" if k.endswith(".svg") else "image")} for k, v in sorted(files.items())]
fixture = {
    "schema_version": 1,
    "fixture_id": "svg-images",
    "suite": "resources-svg",
    "title": "Static SVG images: natural size, viewBox, text, currentColor, nesting, data: glyphs, inline svg",
    "status": "proposed",
    "owner_feature": "007",
    "entry": "index.html",
    "sources": sources,
    "tags": {"standards": ["SVG 2 static", "CSS Images 3"], "u4t": ["U4T-225", "U4T-226"], "contracts": ["U4C-RES", "U4C-BOX", "U4C-RND"], "stages": ["parse", "box", "layout", "raster"]},
    "environment": {"profile": "default"},
    "variants": [variant("scale-1-0", 1), variant("scale-1-25", 1.25), variant("scale-1-5", 1.5), variant("scale-2-0", 2)],
    "observables": ["dom", "geometry", "pixels"],
    "probes": probes,
    "baselines": {},
}
eb = json.dumps({"schema_version": 1, "suite": "resources-svg", "note": "engine-only data (file/data: hrefs, nesting, SVGZ); materialised by tests/svg.rs once the SVG adapter is unblocked", "files": engine}, indent=1) + "\n"
open(os.path.join(HERE, "engine-corpus.json"), "w").write(eb)
open(os.path.join(D, "fixture.json"), "w").write(json.dumps(fixture, indent=2) + "\n")

big = '''#!/usr/bin/env python3
"""Writes an SVG with N (default 1,000,001) <rect> elements to stdout: above the 1,000,000-node limit (SIZE_LIMIT)."""
import sys
n = int(sys.argv[1]) if len(sys.argv) > 1 else 1_000_001
out = sys.stdout
out.write('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">')
for _ in range(n):
    out.write('<rect width="1" height="1"/>')
out.write('</svg>')
'''
open(os.path.join(HERE, "big-svg.py"), "w").write(big)

manifest = {"schema_version": 1, "suite": "resources-svg", "owner_feature": "007", "status": "proposed", "generator": "generate.py",
            "licence": "generated test data; the two data: glyphs are short paraphrases of the Bootstrap 5 (MIT) form-check and form-select icons",
            "files": [{"path": "engine-corpus.json", "sha256": hashlib.sha256(eb.encode()).hexdigest()}] + [{"path": "svg-images/" + k, "sha256": hashlib.sha256(v).hexdigest(), "bytes": len(v)} for k, v in sorted(files.items())]}
open(os.path.join(HERE, "corpus-manifest.json"), "w").write(json.dumps(manifest, indent=1) + "\n")
print(len(files), "files")
