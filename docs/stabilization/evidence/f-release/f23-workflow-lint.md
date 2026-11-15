# F-23 / P9: workflow lint before and after (2026-11-15)

Commands: `uvx zizmor@1.32.0 --offline --persona=auditor --format=plain .github/` (and `--persona=regular`, which CI runs); `docker run rhysd/actionlint:1.7.12@sha256:b1934ee5… -no-color` (shellcheck included in the image).

| finding | baseline (1e49a6a) | this branch (auditor) |
|---|---|---|
| `error[template-injection]` | 2 | 0 |
| `error[unpinned-uses]` | 42 | 0 |
| `help[concurrency-limits]` | 4 | 1 |
| `help[undocumented-permissions]` | 3 | 0 |
| `info[anonymous-definition]` | 15 | 16 |
| `info[template-injection]` | 1 | 0 |
| `warning[artipacked]` | 3 | 0 |

Summary lines:

* baseline: `70 findings (6 unsafe fixes): 16 informational, 7 low, 3 medium, 44 high`
* branch, auditor persona: `17 findings: 16 informational, 1 low, 0 medium, 0 high`
* branch, regular persona (the CI gate): `No findings to report. Good job! (17 suppressed)`
* actionlint: no findings on baseline or branch.

The remaining auditor-only items are informational (`anonymous-definition`: jobs without a `name:`) and one `concurrency-limits` help on the reusable build-dist.yml. The two baseline template injections (publish-powershell.yaml:215-216) are gone: values reach shell scripts only through `env:`, enforced by `test_no_expressions_inside_shell_scripts`.
