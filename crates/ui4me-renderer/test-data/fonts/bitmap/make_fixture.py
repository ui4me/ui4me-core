"""Regenerate the tiny original UI4ME sbix/PNG regression font (fontTools required).
The generated font and this source are dedicated to the public domain (CC0-1.0).
No third-party glyph outlines or images are embedded.
"""
from pathlib import Path
import struct
import zlib
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables.sbixStrike import Strike
from fontTools.ttLib.tables.sbixGlyph import Glyph


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def build(destination):
    # Opaque red pixels make successful PNG decoding distinguishable from black outlines.
    png = b'\x89PNG\r\n\x1a\n'
    png += chunk(b'IHDR', struct.pack('>2I5B', 8, 8, 8, 6, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress((b'\0' + b'\xff\0\0\xff' * 8) * 8))
    png += chunk(b'IEND', b'')
    fb = FontBuilder(1000, isTTF=True)
    order = ['.notdef', 'space', 'bitmap']
    fb.setupGlyphOrder(order)
    glyphs = {}
    for name in order:
        pen = TTGlyphPen(None)
        if name == 'bitmap':
            pen.moveTo((0, 0))
            pen.lineTo((1000, 0))
            pen.lineTo((1000, 1000))
            pen.lineTo((0, 1000))
            pen.closePath()
        glyphs[name] = pen.glyph()
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({name: (1000, 0) for name in order})
    fb.setupHorizontalHeader(ascent=1000, descent=0)
    fb.setupCharacterMap({0x20: 'space', 0xE000: 'bitmap'})
    fb.setupNameTable({'familyName': 'UI4ME Bitmap Test', 'styleName': 'Regular',
        'uniqueFontIdentifier': 'UI4ME Bitmap Test 1', 'fullName': 'UI4ME Bitmap Test Regular',
        'psName': 'UI4MEBitmapTest-Regular', 'version': 'Version 1.000'})
    fb.setupOS2(sTypoAscender=1000, sTypoDescender=0, usWinAscent=1000, usWinDescent=0)
    fb.setupPost()
    table = newTable('sbix')
    strike = Strike(ppem=8, resolution=72)
    strike.glyphs['bitmap'] = Glyph(glyphName='bitmap', originOffsetX=0, originOffsetY=0,
        graphicType='png ', imageData=png)
    table.strikes = {8: strike}
    fb.font['sbix'] = table
    fb.font.recalcTimestamp = False
    fb.font['head'].created = fb.font['head'].modified = 2082844800
    fb.save(destination)


if __name__ == '__main__':
    build(Path(__file__).with_name('UI4MEBitmapTest.ttf'))
