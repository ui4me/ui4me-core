"""Final-binary probe suite: N03 N07 N04 N06 N05 N13."""
import lab, ax, time, json, subprocess, os
from gi.repository import Atspi
R = '/home/toor/projects/ui4me-core'
E = R + '/verification/core-1.0-linux-final/evidence'
HOST = os.path.expanduser('~/.cargo/bin/ui4me-host')
APP = os.path.expanduser('~/.local/share/ui4me-core1-probe/app.toml')
S = '/tmp/claude-1000/-home-toor-projects-ui4me-core/d6bab0d7-7bb7-4a6c-b84c-a5812eceb36c/scratchpad'
M, U = 'Python reference', 'Unicode'
def P(*a): print(*a, flush=True)
def js(cap, code): lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ var w=ws[i]; if (w.caption.indexOf(%s)>=0){ %s; } } OUT('x')" % (json.dumps(cap), code))
def wlcopy(text=None, stdin=None): return subprocess.Popen(['wl-copy'] + ([text] if text else []), stdin=stdin, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
def launch(logname):
    log = open(f'{E}/{logname}', 'w'); p = subprocess.Popen([HOST, 'run', '--app', APP], cwd=os.path.expanduser('~'), stdout=log, stderr=subprocess.STDOUT)
    lab.wait_window(M); lab.wait_window(U); time.sleep(1); return p, log
def out(logname): return [json.loads(l)['fields'].get('text', '') for l in open(f'{E}/{logname}') if 'backend output' in l]

P('## N13 drag and drop')
subprocess.Popen(['dolphin', '--new-window', os.path.expanduser('~/ui4me-dnd')], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
subprocess.Popen(['kwrite', os.path.expanduser('~/ui4me-dnd/source.txt')], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
p, log = launch('N13-probe-run.log'); time.sleep(3)
lab.set_geometry('ui4me-dnd — Dolphin', 100, 100, 800, 600); lab.set_geometry(M, 1000, 100, 900, 590); lab.set_geometry(U, 1500, 700, 500, 300)
k = [w for w in lab.windows() if 'source.txt' in w['caption']][0]['caption']; lab.set_geometry(k, 100, 720, 800, 300); time.sleep(1)
d = Atspi.get_desktop(0); item = None
for i in range(d.get_child_count()):
    a = d.get_child_at_index(i)
    if a.get_name() == 'dolphin': item = ax.find(a, 'list item', 'žluťoučký 日本.txt') or item
e = Atspi.Component.get_extents(item, Atspi.CoordType.WINDOW); w = lab.window('ui4me-dnd — Dolphin')
pw = lab.window(M); tx, ty = pw['cx'] + 450, pw['cy'] + 300
lab.activate('ui4me-dnd — Dolphin'); time.sleep(0.5); lab.dnd(w['cx'] + e.x + 60, w['cy'] + e.y + e.height / 2, tx, ty)
lab.screenshot(f'{E}/N13-after-file-drop.png')
lab.activate(k); time.sleep(0.3); lab.combo('KEY_LEFTCTRL', 'KEY_A'); time.sleep(0.2); ed = None
for i in range(d.get_child_count()):
    a = d.get_child_at_index(i)
    if a.get_name() in ('kwrite', 'KWrite'): ed = ax.find(a, pred=lambda x: x.get_text_iface() is not None and 'drag this' in Atspi.Text.get_text(x, 0, -1)) or ed
r = Atspi.Text.get_character_extents(ed, 5, Atspi.CoordType.WINDOW); kw = lab.window(k)
lab.dnd(kw['cx'] + r.x, kw['cy'] + r.y + r.height / 2, tx, ty); lab.screenshot(f'{E}/N13-after-text-drop.png')
P('drops', [t for t in out('N13-probe-run.log') if 'dropped' in t], '| KWrite source', repr(Atspi.Text.get_text(ed, 0, -1)))
lab.activate(M); time.sleep(0.4); lab.combo('KEY_LEFTCTRL', 'KEY_Q'); p.wait(20); log.close()
for cap in ('source.txt', 'ui4me-dnd — Dolphin'): js(cap, 'w.closeWindow()')
P('DONE')
