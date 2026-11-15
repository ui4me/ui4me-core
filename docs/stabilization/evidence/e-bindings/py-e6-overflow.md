# F-29 Python: audit probe e6_overflow.py against the real headless host

Host: target/debug/ui4me-host built from this branch (host unchanged). UI4ME_TEST_MODE=1 (headless).

```text
--- baseline (python/ at 1e49a6a)
dropped so far: 191
closed events seen by listener: []
w2.closed = False | host windows: ['w1']
RpcError instead of StaleDocumentError: -32004: Window does not exist
app._windows keys: ['w1', 'w2']
--- this branch
dropped so far: 0
closed events seen by listener: ['w2']
w2.closed = True | host windows: ['w1']
StaleDocumentError This document was replaced, unloaded, or closed
app._windows keys: ['w1']
```
