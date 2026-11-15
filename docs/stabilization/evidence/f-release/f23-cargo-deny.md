# F-23 / P8: cargo deny (2026-11-15, cargo-deny 0.20.2, local)

* Baseline: no deny.toml; `cargo deny --locked check advisories` with the default config reports the 4 unmaintained advisories (audit §1 #11).
* With deny.toml but an empty ignore list (control):

```
error[unmaintained]: `ttf-parser` is unmaintained
error[unmaintained]: `zbus_names` is unmaintained
error[unmaintained]: `zvariant` is unmaintained
error[unmaintained]: `zvariant_derive` is unmaintained
advisories FAILED
```

* With deny.toml (written ignores for exactly those 4 IDs): `cargo deny --locked check advisories bans licenses sources` exits 0:

```
advisories ok, bans ok, licenses ok, sources ok
```

bans reports 12 duplicate-version warnings (not fatal by policy). sources: crates.io only; the vendored upstreams are path dependencies.
