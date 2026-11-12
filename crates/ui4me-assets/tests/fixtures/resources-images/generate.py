#!/usr/bin/env python3
"""Regenerates the resources-images fixtures (007 T017).

Layout (002 validate-fixtures treats every sub-directory of a suite as a fixture directory):
  natural-sizes/        an HTML fixture with REAL image containers (PNG, JPEG+EXIF, GIF, BMP, WebP, APNG,
                        16-bit PNG, ICC JPEG, TIFF/AVIF unsupported, truncated, text-named, srcset 1x/2x)
                        for the oracle's geometry capture (OC task T020) and, once a codec adapter is
                        locked (X-ACQ), for engine decoding tests.
  fake-corpus.json      engine-only lifecycle data for the deterministic fake decoder (decode/fake.rs): a
                        map path -> {hex|text}; tests materialise it into a temporary directory.
  corpus-manifest.json  SHA-256 of every generated file.
Codec fixtures are produced with Pillow (authoring tool only; never a project dependency).
Run:  python3 -I generate.py   (from this directory)
"""
import hashlib, io, json, os, struct

HERE = os.path.dirname(os.path.abspath(__file__))
NS = os.path.join(HERE, "natural-sizes")
files = {}


def put(rel, data):
    p = os.path.join(NS, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "wb").write(data)
    files[rel] = data


def fake(w, h, delay_ms=0, fail=0, flags=0, orientation=1, rgba=(255, 0, 0, 255)):
    return b"U4FK" + struct.pack("<III", w, h, delay_ms) + bytes([fail, flags, orientation]) + bytes(rgba)


# ---- fake corpus (engine lifecycle) ------------------------------------------------------------
corpus = {}


def cfile(rel, data):
    corpus[rel] = {"hex": data.hex()} if isinstance(data, bytes) and not rel.endswith((".css", ".html")) else {"text": data.decode()}


cfile("fake/red-4x3.fake", fake(4, 3))
cfile("fake/blue-8x6.fake", fake(8, 6, rgba=(0, 0, 255, 255)))
cfile("fake/green-16x12.fake", fake(16, 12, rgba=(0, 255, 0, 255)))
cfile("fake/delayed-300ms.fake", fake(4, 3, delay_ms=300))
cfile("fake/delayed-600ms.fake", fake(8, 6, delay_ms=600))
cfile("fake/fails-decode.fake", fake(4, 3, fail=1))
cfile("fake/animated.fake", fake(4, 3, flags=2))
cfile("fake/icc.fake", fake(4, 3, flags=4))
cfile("fake/panics.fake", fake(4, 3, flags=1))
cfile("fake/orient6-6x2.fake", fake(6, 2, orientation=6))
cfile("fake/shared-logo.fake", fake(40, 20, rgba=(10, 20, 30, 255)))
cfile("fake/logo@1x.fake", fake(100, 50))
cfile("fake/logo@2x.fake", fake(200, 100))
cfile("css/site.css", b".hero { background-image: url(../fake/green-16x12.fake); }\n.icon { background-image: url('../fake/red-4x3.fake'); }\n")
cfile("index.html", b'<!doctype html><title>resources-images</title><img src="fake/red-4x3.fake"><img srcset="fake/logo@1x.fake 1x, fake/logo@2x.fake 2x">\n')
corpus_body = json.dumps({"schema_version": 1, "suite": "resources-images", "note": "materialised by tests/lifecycle.rs", "files": corpus}, indent=1) + "\n"
open(os.path.join(HERE, "fake-corpus.json"), "w").write(corpus_body)

# ---- codec containers (real) -------------------------------------------------------------------
from PIL import Image, ImageCms  # noqa: E402


def img(w, h, mode="RGB"):
    im = Image.new(mode, (w, h))
    px = im.load()
    for y in range(h):
        for x in range(w):
            v = (x * 255 // max(1, w - 1), y * 255 // max(1, h - 1), (x + y) % 256)
            px[x, y] = v if mode == "RGB" else (*v, 128 + (x % 2) * 100)
    return im


def save(im, fmt, **kw):
    b = io.BytesIO()
    im.save(b, fmt, **kw)
    return b.getvalue()


put("codecs/png-200x100.png", save(img(200, 100), "PNG"))
put("codecs/png16-8x8.png", save(Image.new("I;16", (8, 8), 40000), "PNG"))
frames = [img(32, 16), img(32, 16).transpose(Image.FLIP_LEFT_RIGHT), img(32, 16).transpose(Image.FLIP_TOP_BOTTOM)]
put("codecs/apng-anim.png", save(frames[0], "PNG", save_all=True, append_images=frames[1:], duration=100, loop=0))
exif = Image.Exif()
exif[0x0112] = 6
put("codecs/jpeg-progressive-exif6-120x80.jpg", save(img(120, 80), "JPEG", progressive=True, exif=exif.tobytes(), quality=80))
icc = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
put("codecs/jpeg-icc-64x48.jpg", save(img(64, 48), "JPEG", icc_profile=icc, quality=80))
put("codecs/gif-animated-24x24.gif", save(img(24, 24).convert("P"), "GIF", save_all=True, append_images=[img(24, 24).transpose(Image.FLIP_LEFT_RIGHT).convert("P")], duration=100, loop=0))
put("codecs/webp-lossy-64x32.webp", save(img(64, 32), "WEBP", quality=70, lossless=False))
put("codecs/webp-lossless-64x32.webp", save(img(64, 32), "WEBP", lossless=True))
put("codecs/webp-alpha-64x32.webp", save(img(64, 32, "RGBA"), "WEBP", lossless=False, quality=70))
put("codecs/webp-animated-32x16.webp", save(frames[0], "WEBP", save_all=True, append_images=frames[1:], duration=100, loop=0))
put("codecs/bmp-32x16.bmp", save(img(32, 16), "BMP"))
put("codecs/unsupported-tiff-16x16.tiff", save(img(16, 16), "TIFF"))
put("codecs/unsupported-avif.avif", b"\x00\x00\x00\x1cftypavif\x00\x00\x00\x00avifmif1miaf" + b"\x00" * 32)
put("codecs/truncated.png", files["codecs/png-200x100.png"][:60])
put("codecs/text-named.png", b"this is plain text, not an image\n")
put("codecs/logo@1x.png", save(img(100, 50), "PNG"))
put("codecs/logo@2x.png", save(img(200, 100), "PNG"))

ids = [
    ("png", "codecs/png-200x100.png"), ("png16", "codecs/png16-8x8.png"), ("apng", "codecs/apng-anim.png"),
    ("jpeg-exif6", "codecs/jpeg-progressive-exif6-120x80.jpg"), ("jpeg-icc", "codecs/jpeg-icc-64x48.jpg"),
    ("gif-anim", "codecs/gif-animated-24x24.gif"), ("webp-lossy", "codecs/webp-lossy-64x32.webp"),
    ("webp-lossless", "codecs/webp-lossless-64x32.webp"), ("webp-alpha", "codecs/webp-alpha-64x32.webp"),
    ("webp-anim", "codecs/webp-animated-32x16.webp"), ("bmp", "codecs/bmp-32x16.bmp"),
    ("tiff", "codecs/unsupported-tiff-16x16.tiff"), ("avif", "codecs/unsupported-avif.avif"),
    ("truncated", "codecs/truncated.png"), ("text-png", "codecs/text-named.png"),
]
html = ['<!doctype html>', '<meta charset="utf-8">', '<title>resources-images natural sizes</title>',
        '<style>body{margin:8px;font:12px sans-serif} img{display:block;margin:2px 0}</style>']
for i, p in ids:
    html.append(f'<img id="{i}" data-probe="{i}" src="{p}">')
html.append('<img id="srcset" data-probe="srcset" srcset="codecs/logo@1x.png 1x, codecs/logo@2x.png 2x">')
html.append('<img id="srcset-width" data-probe="srcset-width" width="50" srcset="codecs/logo@1x.png 1x, codecs/logo@2x.png 2x">')
put("index.html", ("\n".join(html) + "\n").encode())

sources = [{"path": k, "sha256": hashlib.sha256(v).hexdigest(), "role": "html" if k.endswith(".html") else "image"} for k, v in sorted(files.items())]
probes = [{"probe_id": i, "observables": ["geometry"], "geometry_class": "G1"} for i, _ in ids] + [
    {"probe_id": "srcset", "observables": ["geometry"], "geometry_class": "G1"},
    {"probe_id": "srcset-width", "observables": ["geometry"], "geometry_class": "G1"},
]
variant = lambda vid, dpr: {"variant_id": vid, "viewport": {"width": 480, "height": 800}, "dpr": dpr, "color_scheme": "light", "reduced_motion": "no-preference"}
fixture = {
    "schema_version": 1,
    "fixture_id": "natural-sizes",
    "suite": "resources-images",
    "title": "Images: natural sizes of every supported and unsupported container, srcset density",
    "status": "proposed",
    "owner_feature": "007",
    "entry": "index.html",
    "sources": sources,
    "tags": {"standards": ["HTML img", "CSS Images 3 natural dimensions"], "u4t": ["U4T-222", "U4T-224"], "contracts": ["U4C-RES", "U4C-BOX", "U4C-LAY"], "stages": ["parse", "box", "layout"]},
    "environment": {"profile": "default"},
    "variants": [variant("dpr1", 1), variant("dpr2", 2)],
    "observables": ["dom", "geometry"],
    "probes": probes,
    "baselines": {},
}
open(os.path.join(NS, "fixture.json"), "w").write(json.dumps(fixture, indent=2) + "\n")

manifest = {
    "schema_version": 1, "suite": "resources-images", "owner_feature": "007", "status": "proposed",
    "generator": "generate.py (Pillow, authoring only)", "licence": "generated test data, no third-party content",
    "lifecycle_scenarios_in": "engine/crates/ui4me-resources/tests/lifecycle.rs and tests/deterministic.rs",
    "files": [{"path": "fake-corpus.json", "sha256": hashlib.sha256(corpus_body.encode()).hexdigest()}]
    + [{"path": "natural-sizes/" + k, "sha256": hashlib.sha256(v).hexdigest(), "bytes": len(v)} for k, v in sorted(files.items())],
}
open(os.path.join(HERE, "corpus-manifest.json"), "w").write(json.dumps(manifest, indent=1) + "\n")
print(len(files), "fixture files")
