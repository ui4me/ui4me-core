# F-10 CI gate: live read-only check against the GitHub Actions API (2026-11-15)

Command: `GH_TOKEN=$(gh auth token) python3 tools/release/ci_gate.py --repo ui4me/ui4me-core --sha <SHA> --config tools/release/testdata/ci_gate/required-1.0.x.json`

## d053e09478a081a48b68782f2204c4f649ae0e3b

```
CI gate REFUSED: ci.yml: the CI run for d053e09478a081a48b68782f2204c4f649ae0e3b concluded failure (https://github.com/ui4me/ui4me-core/actions/runs/43484564824)
```

## c6fd71dc5c15dcdfdcdc1500dc156c1b929c216c

```
{
  "result": "ACCEPTED",
  "sha": "c6fd71dc5c15dcdfdcdc1500dc156c1b929c216c",
  "workflows": [
    {
      "workflow": "ci.yml",
      "run_id": 43551153042,
      "run_attempt": 1,
      "url": "https://github.com/ui4me/ui4me-core/actions/runs/43551153042",
      "jobs": [
        "verify (ubuntu-24.04)",
        "verify (windows-2022)"
      ]
    }
  ]
}
```

## 1e49a6a000000000000000000000000000000000

```
CI gate REFUSED: ci.yml: no push run for 1e49a6a000000000000000000000000000000000 on main. Push the commit and wait for CI before publishing.
```

d053e09 is the commit 1.0.0 was published from (run 43484619109) while its CI run 43484564824 failed on Windows: refused (exit 1). c6fd71d (1.0.1) is accepted (exit 0). A SHA with no CI run is refused (exit 1). On the baseline no such gate existed: publish.yaml never queried CI.
