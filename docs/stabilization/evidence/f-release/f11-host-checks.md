# F-11 interim / RELEASE-17: host checks against real binaries (2026-11-15)

Published 1.0.1 hosts (PSGallery module install, hashes equal the audited 8d7eab92…/be08dd88…), and the local debug host:

```
win-x64/ui4me_core/_bin/ui4me-host.exe sha256 8d7eab92c21bdb4e617e3f645e83f0aa9833226f4356307be4f6caff5d79a7cb bytes 30202880
  SizeOfStackReserve 1048576 expected 33554432
  debug markers []
linux-x64/ui4me_core/_bin/ui4me-host sha256 be08dd8837158694c0a348f3a2a54b8815edcff648424e7630667fc8d64e9136 bytes 41029064
  debug markers []
target/debug/ui4me-host bytes 344750568 debug markers ['344750568 bytes exceeds the release-host bound 157286400', 'contains debug sections .debug_abbrev, .debug_aranges, .debug_gdb_scripts, .debug_info', 'contains Rust debug-assertion checks (built without the release profile)']
```

* The published 1.0.1 Windows host reserves 1 MiB: `verify_windows_artifact.validate_wheel` now refuses it (baseline accepted it: `pe_header` read only machine and subsystem).
* No false positive: neither published release host shows a debug marker.
* The debug host trips all three markers; `setup.py` with `UI4ME_WHEEL_HOST_BINARY=target/debug/ui4me-host` now raises "is a development host" (baseline packaged it silently via `UI4ME_HOST_BINARY`).
