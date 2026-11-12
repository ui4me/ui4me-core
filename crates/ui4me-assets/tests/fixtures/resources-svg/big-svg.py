#!/usr/bin/env python3
"""Writes an SVG with N (default 1,000,001) <rect> elements to stdout: above the 1,000,000-node limit (SIZE_LIMIT)."""
import sys
n = int(sys.argv[1]) if len(sys.argv) > 1 else 1_000_001
out = sys.stdout
out.write('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">')
for _ in range(n):
    out.write('<rect width="1" height="1"/>')
out.write('</svg>')
