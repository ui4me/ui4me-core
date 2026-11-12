#!/usr/bin/env python3
"""Regenerates the WOFF2 test fixtures (015 TXT-1, D-LINL-05) from the pinned OFL test fonts.

Run offline:  python3 -I generate.py <fonttools-site-dir> <repo-root>
(`pip install --target <dir> fonttools brotli`; the fixtures record fontTools 4.68.0, brotli 1.2.1.)
Each fixture is a Latin subset (U+0020-007E, U+00C0-00FF, U+2022) of a pinned font, kept hinted, converted with
fontTools' WOFF2 writer. Outputs: the .ttf subsets (the comparison originals) and the .woff2 files.
  noto-sans-subset.woff2          glyf/loca transformed (default), composites with instructions
  noto-sans-hmtx.woff2            glyf/loca/hmtx transformed (section 5.4)
  noto-sans-nulltransform.woff2   glyf/loca stored untransformed (transform version 3)
  roboto-flex-subset.woff2        variable (gvar, avar, fvar), overlapping contours (overlap bitmap)
  bomb-declared.woff2             one table declaring 128 MiB (over the font limit; refused before any Brotli work)
  bomb-stream.woff2               one table declaring 1000 bytes whose Brotli stream expands to 32 MiB of zeros
  bomb-ratio.woff2                one table declaring 48 MiB (inside the limit) that really expands to it: decodes only
                                  under a larger cap, SIZE_LIMIT under a smaller one
"""
import hashlib, json, os, sys
site, root = sys.argv[1], sys.argv[2]
sys.path.insert(0, site)
from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.ttLib.woff2 import WOFF2FlavorData

here = os.path.dirname(os.path.abspath(__file__))
fonts = os.path.join(root, "verification/fixtures/fonts")
sources = {"noto-sans": "noto-sans/NotoSans-Regular.ttf", "roboto-flex": "roboto-flex/RobotoFlex-VF.ttf"}


def sub(src):
    opts = subset.Options()
    opts.layout_features = ["*"]
    opts.notdef_outline = True
    opts.glyph_names = False
    opts.name_IDs = ["*"]
    f = TTFont(os.path.join(fonts, src))
    s = subset.Subsetter(opts)
    s.populate(unicodes=list(range(0x20, 0x7F)) + list(range(0xC0, 0x100)) + [0x2022])
    s.subset(f)
    return f


def save(f, name, flavor_data=None, keep_ttf=True):
    f.flavor = None
    path = os.path.join(here, name + ".ttf")
    f.save(path)
    g = TTFont(path)
    g.flavor = "woff2"
    if flavor_data is not None:
        g.flavorData = flavor_data
    g.save(os.path.join(here, name + ".woff2"))
    if not keep_ttf:
        os.remove(path)


save(sub(sources["noto-sans"]), "noto-sans-subset")
# the hmtx and null-transform variants share the Noto original
save(sub(sources["noto-sans"]), "noto-sans-hmtx", WOFF2FlavorData(transformedTables={"glyf", "loca", "hmtx"}), keep_ttf=False)
save(sub(sources["noto-sans"]), "noto-sans-nulltransform", WOFF2FlavorData(transformedTables=set()), keep_ttf=False)
save(sub(sources["roboto-flex"]), "roboto-flex-subset")

import brotli


def base128(n):
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.append(0x80 | (n & 0x7F))
        n >>= 7
    return bytes(reversed(out))


def raw_woff2(tag_index_value, declared, payload, total_sfnt=0):
    """A one-table WOFF2 container: `declared` is the directory origLength, `payload` the table data."""
    comp = brotli.compress(payload, quality=5)
    entry = bytes([tag_index_value]) + base128(declared)
    body = entry + comp
    import struct
    head = b"wOF2" + struct.pack(">IIHHIIHHIIIII", 0x00010000, 48 + len(body), 1, 0, total_sfnt, len(comp), 1, 0, 0, 0, 0, 0, 0)
    assert len(head) == 48
    return head + body


CVT = 8  # known-tag index of "cvt "
open(os.path.join(here, "bomb-declared.woff2"), "wb").write(raw_woff2(CVT, 128 << 20, b"\0" * 4096))
open(os.path.join(here, "bomb-stream.woff2"), "wb").write(raw_woff2(CVT, 1000, b"\0" * (32 << 20)))
open(os.path.join(here, "bomb-ratio.woff2"), "wb").write(raw_woff2(CVT, 48 << 20, b"\0" * (48 << 20)))

prov = {
    "schema_version": 1,
    "generator": "generate.py",
    "tools": {"fonttools": "4.68.0", "brotli": "1.2.1"},
    "sources": {k: {"path": "verification/fixtures/fonts/" + v, "sha256": hashlib.sha256(open(os.path.join(fonts, v), "rb").read()).hexdigest(), "licence": "OFL-1.1"} for k, v in sources.items()},
    "files": {n: hashlib.sha256(open(os.path.join(here, n), "rb").read()).hexdigest() for n in sorted(os.listdir(here)) if n.endswith((".ttf", ".woff2"))},
}
open(os.path.join(here, "provenance.json"), "w").write(json.dumps(prov, indent=1) + "\n")
