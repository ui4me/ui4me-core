# Developing UI4ME Core itself

Application authors should normally install the binary wheel, not build the core. This page is for people modifying the Rust/Python runtime.

## Checkout and build

```bash
git clone https://github.com/ui4me/ui4me-core.git
cd ui4me-core
bash tools/bootstrap_linux.sh --install-system
python3 tools/run_renderer.py
```

On Windows install MSVC build tools, the pinned Rust toolchain and Python. Run GUI tests from an interactive desktop. Use `tools/bootstrap_windows.ps1` and `tools/run_renderer.py` as documented in the qualification guides.

## Tests

```bash
python3 tools/verify.py
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 -m unittest discover -s tools -p 'test_*.py'
```

Full native acceptance is documented in [CORE_1.0_ACCEPTANCE.md](../CORE_1.0_ACCEPTANCE.md). Headless rendering and source compilation do not establish native GUI correctness. Preserve the historical verification files; source changes invalidate previously measured binary hashes.

## Architecture

The Rust host owns windows and application lifecycle. The document, renderer, asset and shared-type crates implement the HTML/CSS engine. The Python client is separate and uses typed JSON-RPC over an authenticated private local connection. For continuous pixels, the host manages bounded shared file mappings rather than encoding every frame into IPC.

Important constraints: never wait synchronously for Python from the native GUI loop; distinguish style/layout/paint-only invalidation; validate document/node generations after replacement; bound resource and transport queues; use native keyboard, IME and accessibility semantics; track dependency patch provenance.

[Core specification](../CORE_1.0_SPEC.md) · [Native host protocol](../PROTOCOL.md) · [Document commands](../PROTOCOL_DOCUMENTS.md) · [Third-party notices](../../THIRD_PARTY_NOTICES.md).
