#!/usr/bin/env python3
"""Regenerates the resources-fonts corpus (007 T054).

002's validate-fixtures treats every sub-directory of a suite as an HTML fixture directory, so this
suite keeps its engine-only data as top-level files:
  synthetic-fonts.json  stub/invalid/synthetic container bytes (hex), materialised by tests/fonts.rs
  faces.json            @font-face scenarios (descriptors, sources, expected states)
  corpus-manifest.json  SHA-256 of the above
The only real font is the pinned Noto Sans in ../fonts/noto-sans/ (002-owned, read-only; R-46),
referenced by path and SHA-256, never copied here.
Run:  python3 -I generate.py   (from this directory)
"""
import hashlib, json, os, struct

HERE = os.path.dirname(os.path.abspath(__file__))
fonts = {}


def sfnt(tag, tables, total):
    """Minimal sfnt: directory of (tag, offset, length) records, zero padded to `total` bytes."""
    v = tag + struct.pack(">HHHH", len(tables), 0, 0, 0)
    for t, off, ln in tables:
        v += t + struct.pack(">III", 0, off, ln)
    return v + b"\0" * (total - len(v))


fonts["woff2-stub.woff2"] = b"wOF2" + b"\0" * 44
fonts["woff-stub.woff"] = b"wOFF" + b"\0" * 44
fonts["invalid.ttf"] = b"this is not a font file at all\n"
fonts["truncated.ttf"] = sfnt(b"\0\1\0\0", [(b"head", 28, 4096)], 64)
fonts["zero.ttf"] = b""
dir_end = 12 + 16
fonts["synthetic-collection.ttc"] = b"ttcf" + struct.pack(">III", 0x00010000, 1, 16) + sfnt(b"\0\1\0\0", [(b"head", 16 + dir_end, 8)], dir_end + 8 + 8)

synthetic = {"schema_version": 1, "suite": "resources-fonts", "files": {k: {"hex": v.hex()} for k, v in sorted(fonts.items())}}
synthetic_body = json.dumps(synthetic, indent=1) + "\n"
open(os.path.join(HERE, "synthetic-fonts.json"), "w").write(synthetic_body)

faces = {
    "schema_version": 1,
    "note": "Rule descriptors used by tests/fonts.rs; sources name synthetic files or the pinned Noto Sans (NOTO).",
    "pinned_font": {"path": "verification/fixtures/fonts/noto-sans/NotoSans-Regular.ttf", "sha256": "89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d"},
    "scenarios": [
        {"id": "src-order-woff2-then-ttf", "family": "Fixture A", "sources": ["url(woff2-stub.woff2) format(woff2)", "url(NOTO) format(truetype)"], "expect": "Loaded", "reads": 1},
        {"id": "invalid-then-valid", "family": "Fixture B", "sources": ["url(invalid.ttf)", "url(NOTO)"], "expect": "Loaded"},
        {"id": "all-fail", "family": "Fixture C", "sources": ["url(invalid.ttf)", "url(woff-stub.woff)", "url(missing.ttf)"], "expect": "Failed", "codes": ["DECODE_FAILED", "UNSUPPORTED_MEDIA_TYPE", "FILE_NOT_FOUND"]},
        {"id": "unicode-range-subset", "family": "Fixture D", "unicode_range": [[0, 255], [8192, 8303]], "sources": ["url(NOTO)"]},
        {"id": "weight-range", "family": "Fixture E", "weight": [100, 900], "width": [75, 125], "style": {"oblique": [-20, 20]}, "sources": ["url(NOTO)"]},
        {"id": "font-display-values", "display": ["auto", "block", "swap", "fallback", "optional"]},
        {"id": "ttc-collection", "family": "Fixture F", "sources": ["url(synthetic-collection.ttc)"], "expect": "Loaded"},
        {"id": "local-source", "family": "Fixture G", "sources": ["local(Registered Family)"]},
        {"id": "two-documents-same-family", "family": "Shared Name", "note": "document 1 -> NOTO, document 2 -> synthetic-collection.ttc"},
    ],
}
faces_body = json.dumps(faces, indent=1) + "\n"
open(os.path.join(HERE, "faces.json"), "w").write(faces_body)

manifest = {
    "schema_version": 1,
    "suite": "resources-fonts",
    "owner_feature": "007",
    "title": "Font loading and registration: @font-face sources, format skipping, validation, ranges",
    "status": "proposed",
    "generator": "generate.py",
    "references": [{"path": "verification/fixtures/fonts/noto-sans/NotoSans-Regular.ttf", "sha256": "89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d", "owner": "002 (R-46)", "licence": "OFL-1.1"}],
    "files": [
        {"path": "synthetic-fonts.json", "sha256": hashlib.sha256(synthetic_body.encode()).hexdigest()},
        {"path": "faces.json", "sha256": hashlib.sha256(faces_body.encode()).hexdigest()},
    ],
}
open(os.path.join(HERE, "corpus-manifest.json"), "w").write(json.dumps(manifest, indent=1) + "\n")
print(len(fonts), "synthetic fonts")
