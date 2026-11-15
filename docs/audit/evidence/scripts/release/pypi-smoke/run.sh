#!/bin/sh
set -eu
python -V
pip install -q --disable-pip-version-check --no-cache-dir ui4me-core==1.0.1 2>&1 | tail -2
pip show -f ui4me-core | grep -E "_bin|\.libs|^Version" | head
BIN=$(python -c 'from ui4me_core.launcher import host_binary;print(host_binary())')
echo "host=$BIN"
ldd "$BIN" | sed 's/^/  ldd: /'
ui4me-host --version
cd /tmp
python /smoke/smoke_installed_wheel.py
cat > /tmp/headless_app.py <<'PY'
import threading
from ui4me_core import Application
with Application.start(name='Clean machine check', instance='multiple') as app:
    w = app.open_html("<button id='b'>Go</button><p id='t'>before</p>")
    w.document.set_text('t', 'after')
    print('TEXT', w.document.get_text('t'))
    threading.Timer(0.3, app.request_quit).start()
    app.run()
print('EXIT-OK')
PY
UI4ME_TEST_MODE=1 timeout 60 python /tmp/headless_app.py
mkdir -p /tmp/proj && cd /tmp/proj && (ui4me-new --help 2>&1 | head -5) || true
