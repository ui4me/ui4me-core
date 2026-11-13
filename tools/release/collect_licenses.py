#!/usr/bin/env python3
"""Write the license texts of every crate linked into the `ui4me-host` binary.

A wheel distributes the compiled host, which statically links its Rust
dependencies. Their licenses require the notices to travel with the binary.
This reads `cargo metadata` (locked, Linux x86_64 target), walks the host's
normal and build dependency closure, and concatenates each package's license
files from its unpacked source (registry checkout or vendored `patches/` /
`third_party/` directory). It fails if a package has neither an SPDX license
expression nor at least one license file, so a new dependency cannot ship
silently without its notice.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
LICENSE_PREFIXES = ("license", "licence", "copying", "notice", "copyright", "ofl", "unlicense")
TARGET = "x86_64-unknown-linux-gnu"


def license_files(package: dict) -> list[Path]:
    directory = Path(package["manifest_path"]).parent
    if directory.parent == ROOT / "crates":
        # Workspace crates: the project's MIT license and the bundled font's OFL.
        return [ROOT / "LICENSE", ROOT / "crates" / "ui4me-host" / "assets" / "FONT-LICENSE.txt"]
    found = []
    if package.get("license_file"):
        found.append(directory / package["license_file"])
    for path in sorted(directory.iterdir()):
        name = path.name.lower()
        if path.is_file() and name.startswith(LICENSE_PREFIXES) and path not in found:
            found.append(path)
    return [p for p in found if p.is_file()]


MIT_TEXT = """Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in the
Software without restriction, including without limitation the rights to use, copy,
modify, merge, publish, distribute, sublicense, and/or sell copies of the Software,
and to permit persons to whom the Software is furnished to do so, subject to the
following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE
OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""

BSD2_TEXT = """Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list
   of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this
   list of conditions and the following disclaimer in the documentation and/or other
   materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT
SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR
BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY
WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""


def reference_texts(packages: list[dict]) -> list[tuple[str, str]]:
    """Canonical texts: MIT and BSD-2-Clause templates (copyright: the package
    authors listed in the index), Apache-2.0 and MPL-2.0 from files that ship
    in this tree or the locked registry sources."""
    out = [("MIT (template; copyright holders are each package's authors)", MIT_TEXT),
           ("BSD-2-Clause (template; copyright holders are each package's authors)", BSD2_TEXT),
           ("MPL-2.0", (ROOT / "patches" / "licenses" / "MPL-2.0.txt").read_text(encoding="utf-8"))]
    for package in packages:
        for path in license_files(package):
            if path.name.upper() in ("LICENSE-APACHE", "LICENSE-APACHE.TXT", "LICENSE-APACHE.MD"):
                return out + [("Apache-2.0", path.read_text(encoding="utf-8", errors="replace"))]
    raise SystemExit("No Apache-2.0 reference text found in the dependency sources")


def closure(metadata: dict, root_name: str) -> list[dict]:
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root = next(p["id"] for p in metadata["packages"] if p["name"] == root_name)
    seen, stack = set(), [root]
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            if any(kind["kind"] in (None, "build") for kind in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    return sorted((packages[i] for i in seen), key=lambda p: (p["name"], p["version"]))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--package", default="ui4me-host")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", TARGET],
        cwd=ROOT))
    packages = closure(metadata, args.package)
    missing = []
    sections = [
        "UI4ME Core host — third-party licenses\n",
        f"The `{args.package}` executable in this distribution statically links the "
        f"{len(packages)} Rust packages listed below (resolved from Cargo.lock for {TARGET}).\n"
        "Each package's license texts follow its entry. MPL-2.0 packages (Servo/Stylo) are\n"
        "distributed with their complete source, including the project's patches, in the\n"
        "ui4me-core source distribution (patches/stylo).\n",
        "",
        "Index (name version — license):",
    ]
    for package in packages:
        sections.append(f"  {package['name']} {package['version']} — {package['license'] or 'see license file'}")
    for package in packages:
        files = license_files(package)
        local = package["source"] is None
        if not files and not package["license"]:
            missing.append(f"{package['name']} {package['version']}")
            continue
        sections.append("\n" + "=" * 78)
        sections.append(f"{package['name']} {package['version']}")
        sections.append(f"License: {package['license'] or 'see below'}")
        if package.get("repository"):
            sections.append(f"Repository: {package['repository']}")
        if local:
            sections.append("Source: included in the UI4ME Core source distribution")
        sections.append("=" * 78)
        if not files:
            authors = ", ".join(package.get("authors") or []) or "the package authors"
            sections.append(f"(No license file in the published package. SPDX: {package['license']};"
                            f" copyright {authors}. See the reference texts at the end.)")
        for path in files:
            sections.append(f"--- {path.name} ---")
            sections.append(path.read_text(encoding="utf-8", errors="replace").rstrip())
    if missing:
        print("Packages without license information: " + ", ".join(missing), file=sys.stderr)
        return 1
    sections.append("\n" + "=" * 78)
    sections.append("Reference license texts for packages that publish only an SPDX expression")
    sections.append("=" * 78)
    for title, text in reference_texts(packages):
        sections.append(f"--- {title} ---")
        sections.append(text.rstrip())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(sections) + "\n", encoding="utf-8")
    print(f"{len(packages)} packages -> {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
