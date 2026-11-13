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

P('## N03 / N07')
p, log = launch('N03-probe-run.log')
lab.set_geometry(U, 1200, 300, 600, 420); time.sleep(0.8); w = lab.window(U); P('resize', w['w'], w['h'])
for name, code in (('maximize', 'w.setMaximize(true,true)'), ('unmaximize', 'w.setMaximize(false,false)'), ('fullscreen', 'w.fullScreen=true'), ('leave fullscreen', 'w.fullScreen=false'), ('minimize', 'w.minimized=true'), ('restore', 'w.minimized=false')):
    js(U, code); time.sleep(1); w = lab.window(U); P(name, w['x'], w['y'], w['w'], w['h'], 'max', w['maximized'], 'fs', w['fullScreen'], 'min', w['minimized'])
lab.screenshot(f'{E}/N03-two-windows.png')
js(U, 'w.closeWindow()'); time.sleep(1.5)
P('compositor close of second window ->', [x['caption'] for x in lab.windows() if x['res'] == 'org.ui4me.HostReference'], '| backend', [t for t in out('N03-probe-run.log') if 'Close accepted' in t])
A = ax.app('UI4ME Host Reference'); P('tree', [(d, a.get_role_name(), a.get_name()) for d, a in ax.all_nodes(A)])
lab.activate(M); time.sleep(0.5); order = []
for i in range(6):
    lab.key('KEY_TAB'); time.sleep(0.4); order.append(ax.focused(A))
P('Tab order', order)
paste = ax.find(A, pred=lambda a: a.get_name() == 'Paste'); P('grab_focus(Paste)', Atspi.Component.grab_focus(paste), ax.focused(A))
ed = ax.find(A, 'entry', 'Editable input'); Atspi.Text.set_selection(ed, 0, 0, 6)
copy = ax.find(A, pred=lambda a: a.get_name() == 'Copy'); Atspi.Action.do_action(copy, 0); time.sleep(0.8)
P('Copy action -> wl-paste', repr(subprocess.run(['wl-paste', '-n'], capture_output=True, text=True, timeout=5).stdout))

P('## N04 clipboard')
lab.set_geometry(M, 400, 150, 900, 590); time.sleep(0.8); ax.click(ed, M); time.sleep(0.3)
lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.combo('KEY_LEFTCTRL', 'KEY_C'); time.sleep(0.6)
v = subprocess.run(['wl-paste', '-n'], capture_output=True, text=True, timeout=5).stdout
P('probe -> wl-paste: MIME', subprocess.run(['wl-paste', '-l'], capture_output=True, text=True, timeout=5).stdout.split()[:2], 'equal', v == ax.text(ed), 'len', len(v))
c = wlcopy('Unicode from another app: Příliš žluťoučký kůň — 日本語 — 😀'); time.sleep(0.6)
lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.8); P('wl-copy -> probe', repr(ax.text(ed))); c.kill()
before = ax.text(ed); c = wlcopy(stdin=open(S + '/big.txt')); time.sleep(1.5); lab.key('KEY_END'); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(2)
P('2.2 MB paste: unchanged', ax.text(ed) == before, len(before)); c.kill()

P('## N06 IME + layout')
im = lambda: subprocess.run(['fcitx5-remote', '-n'], capture_output=True, text=True).stdout.strip()
lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.key('KEY_BACKSPACE'); time.sleep(0.3)
P('IM', im()); lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.6); P('IM', im())
lab.type_text('nihao'); time.sleep(1); lab.screenshot(f'{E}/N06-probe-preedit.png'); P('during preedit', repr(ax.text(ed)))
lab.key('KEY_SPACE'); time.sleep(0.6); P('commit', repr(ax.text(ed)), 'caret', Atspi.Text.get_caret_offset(ed))
lab.type_text('shijie'); time.sleep(0.6); lab.key('KEY_ESC'); time.sleep(0.5); P('Escape', repr(ax.text(ed)))
lab.combo('KEY_LEFTSHIFT', 'KEY_LEFT'); time.sleep(0.2); lab.type_text('wen'); time.sleep(0.6); lab.key('KEY_SPACE'); time.sleep(0.6); P('compose over selected char', repr(ax.text(ed)))
lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.5); P('IM back', im())
lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.key('KEY_BACKSPACE')
lab.key('KEY_APOSTROPHE'); lab.key('KEY_E'); lab.combo('KEY_LEFTSHIFT', 'KEY_GRAVE'); lab.key('KEY_N'); lab.key('KEY_APOSTROPHE'); lab.key('KEY_SPACE'); time.sleep(0.6)
P("us-intl dead keys (' e, ~ n, ' space):", repr(ax.text(ed))); lab.screenshot(f'{E}/N06-dead-keys.png')
lab.activate(M); time.sleep(0.4); lab.combo('KEY_LEFTCTRL', 'KEY_Q'); p.wait(20); log.close()

P('## N05 dialogs')
p, log = launch('N05-probe-run.log')
dlg = lambda: [(x['caption'], x['transient'], x['modal'], x['transientFor']) for x in lab.windows() if x['res'].startswith('org.freedesktop.impl.portal')]
last = lambda k: [t for t in out('N05-probe-run.log') if k in t][-1:]
lab.set_geometry(M, 200, 100, 900, 590); lab.set_geometry(U, 1200, 100, 540, 390); time.sleep(0.8)
lab.activate(M); time.sleep(0.4); lab.key('KEY_F3'); time.sleep(3); P('F3', dlg()); lab.screenshot(f'{E}/N05-open-dialog.png'); lab.key('KEY_ESC'); time.sleep(1.5); P('Escape', dlg(), last('dialog.open_files'))
lab.activate(M); time.sleep(0.4); lab.key('KEY_F3'); time.sleep(3); c = wlcopy(os.path.expanduser('~/ui4me-test/žluťoučký 日本.txt')); time.sleep(0.5)
lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.6); lab.key('KEY_ENTER'); time.sleep(2); c.kill(); P('select', dlg(), last('dialog.open_files'))
lab.activate(M); time.sleep(0.4); lab.key('KEY_F4'); time.sleep(3); P('F4', dlg()); c = wlcopy(os.path.expanduser('~/ui4me-test/saved by probe.txt')); time.sleep(0.5)
lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.6); lab.key('KEY_ENTER'); time.sleep(2); c.kill(); P('save', dlg(), last('dialog.save_file'))
lab.activate(M); time.sleep(0.4); lab.key('KEY_F5'); time.sleep(3); P('F5', dlg()); lab.screenshot(f'{E}/N05-folder-dialog.png'); lab.key('KEY_ESC'); time.sleep(1.5); P('Escape', dlg(), last('dialog.choose_folder'))
lab.activate(M); time.sleep(0.4); lab.key('KEY_F3'); time.sleep(3); P('F3 again', dlg()); lab.activate(U); time.sleep(0.6); lab.key('KEY_F7'); time.sleep(2); P('F7 from second window', dlg(), last('dialog.open_files'))
lab.activate(M); time.sleep(0.4); lab.key('KEY_F3'); time.sleep(3); P('F3 then parent close', dlg())
js(M, 'w.closeWindow()'); time.sleep(2.5); P('after parent close', dlg(), last('dialog.open_files'), [x['caption'] for x in lab.windows() if x['res'] == 'org.ui4me.HostReference'])
hp = lab.window(U)['pid']; lab.activate(U); time.sleep(0.4); lab.key('KEY_F5'); time.sleep(3); P('F5 from second window', dlg())
lab.activate(U); time.sleep(0.5); t = time.time(); lab.combo('KEY_LEFTCTRL', 'KEY_Q')
while time.time() - t < 10 and subprocess.run(['kill', '-0', str(hp)], capture_output=True).returncode == 0: time.sleep(0.05)
time.sleep(1); P('Ctrl+Q with folder dialog open: exit after %.2f s, dialogs %s, exit code %s' % (time.time() - t - 1, dlg(), p.wait(20))); log.close()

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
