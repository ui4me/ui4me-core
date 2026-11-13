"""Final-binary reference-app suite: N01 N02 N12 R11 R01 R02 R04 R05 R06 N08 R03 R08 R07."""
import lab, ax, time, subprocess, json, os, sys
from gi.repository import Atspi
E = '/home/toor/projects/ui4me-core/verification/core-1.0-linux-final/evidence'
C = 'Desktop workspace'
out = {}
def P(*a):
    print(*a, flush=True)
def ps():
    o = subprocess.run(['ps', '-eo', 'pid,ppid,args', '--no-headers'], capture_output=True, text=True).stdout
    return [l.strip()[:160] for l in o.splitlines() if ('ui4me-host run' in l or 'renderer_demo.py' in l) and 'ps -eo' not in l and 'systemd-run' not in l]
def kickoff(q, shot=None):
    lab.key('KEY_LEFTMETA'); time.sleep(1.2); lab.type_text(q); time.sleep(1.5)
    if shot: lab.screenshot(shot)
    lab.key('KEY_ENTER'); time.sleep(5)
def wlpaste(*a):
    return subprocess.run(['wl-paste', *a], capture_output=True, text=True, timeout=5).stdout
def wlcopy(text):
    return subprocess.Popen(['wl-copy', text], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
def close(cap):
    lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0) ws[i].closeWindow(); } OUT('x')" % json.dumps(cap))

P('## N01/N02/N12/R11/R01')
P('host sha256', subprocess.check_output(['sha256sum', os.path.expanduser('~/.cargo/bin/ui4me-host')], text=True).split()[0])
kickoff('UI4ME Desktop', f'{E}/N01-kickoff-entry.png')
w = lab.wait_window(C); P('N01 KWin', {k: w[k] for k in ('caption', 'res', 'desktopFile', 'pid')})
for p in ps():
    pid = p.split()[0]; P('  N12', p, '| cwd', os.readlink(f'/proc/{pid}/cwd'), '| exe', os.readlink(f'/proc/{pid}/exe'))
lab.screenshot(f'{E}/R01-window.png'); lab.screenshot(f'{E}/N01-window-taskbar.png')
kickoff('UI4ME Desktop')
P('N02 after second launch: windows', len([x for x in lab.windows() if C in x['caption']]), 'backends', len([p for p in ps() if 'renderer_demo' in p]))

P('## R02')
lab.set_geometry(C, 300, 40, 1200, 1000); time.sleep(1)
A = ax.wait_app('UI4ME Desktop Workspace'); P('AT-SPI app name', A.get_name())
rows = lambda: [[c.get_name() for _, c in ax.all_nodes(r) if c.get_role_name() == 'table cell'] for _, r in ax.all_nodes(A) if r.get_role_name() == 'table row']
status = lambda: [a.get_name() for _, a in ax.all_nodes(ax.find(A, 'status bar')) if a.get_role_name() == 'label']
name = ax.find(A, 'entry', 'Name'); ax.click(name, C); time.sleep(0.2)
lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.type_text('<tag> & value'); time.sleep(0.4); P('name', repr(ax.text(name)))
cb = ax.find(A, 'check box', 'Include notification flag'); b = cb.get_state_set().contains(Atspi.StateType.CHECKED); ax.click(cb, C); time.sleep(0.4); P('checkbox', b, '->', cb.get_state_set().contains(Atspi.StateType.CHECKED))
work = ax.find(A, 'radio button', 'Work'); ax.click(work, C); time.sleep(0.4); P('radio Work', work.get_state_set().contains(Atspi.StateType.CHECKED))
est = ax.find(A, 'spin button', 'Estimate'); ax.click(est, C); lab.key('KEY_UP'); lab.key('KEY_UP'); time.sleep(0.4); P('estimate', Atspi.Value.get_current_value(est))
sl = ax.find(A, 'slider', 'Progress'); ax.click(sl, C); time.sleep(0.2); v0 = Atspi.Value.get_current_value(sl); lab.key('KEY_RIGHT'); lab.key('KEY_RIGHT'); time.sleep(0.4); P('slider', v0, '->', Atspi.Value.get_current_value(sl))
pr = ax.find(A, 'combo box', 'Priority'); ax.click(pr, C); time.sleep(0.6); lab.key('KEY_DOWN'); lab.key('KEY_ENTER'); time.sleep(0.5)
P('priority', [Atspi.Selection.get_selected_child(pr, i).get_name() for i in range(Atspi.Selection.get_n_selected_children(pr))])
P('rows before', rows()); ax.click(ax.find(A, 'button', 'Add row'), C); time.sleep(0.8); P('rows after', rows()[-1], '| status', status())
lab.screenshot(f'{E}/R02-row-added.png')
th = ax.find(A, 'button', 'Change palette'); ax.click(th, C); time.sleep(0.8); lab.screenshot(f'{E}/R02-palette-toggled.png'); ax.click(th, C); time.sleep(0.5)

P('## R04')
ax.click(name, C); time.sleep(0.3); order = [ax.focused(A)]
for i in range(13):
    lab.key('KEY_TAB'); time.sleep(0.35); order.append(ax.focused(A))
P('tab order', order); lab.combo('KEY_LEFTSHIFT', 'KEY_TAB'); time.sleep(0.3); P('shift-tab', ax.focused(A))
ax.click(name, C); lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.key('KEY_BACKSPACE'); time.sleep(0.2); n0 = len(rows())
lab.key('KEY_ENTER'); time.sleep(0.8); P('empty required + Enter: rows', n0, '->', len(rows()), 'focus', ax.focused(A))
lab.screenshot(f'{E}/R04-invalid-required.png')
lab.type_text('Typed item'); lab.key('KEY_ENTER'); time.sleep(0.8); P('implicit submit: last row', rows()[-1])
notes = ax.find(A, 'entry', 'Notes'); ax.click(notes, C); lab.type_text('alpha beta gamma'); time.sleep(0.3)
e = Atspi.Component.get_extents(notes, Atspi.CoordType.WINDOW); w = lab.window(C)
lab.drag(w['cx'] + e.x + 14, w['cy'] + e.y + 16, w['cx'] + e.x + 70, w['cy'] + e.y + 16, steps=10); time.sleep(0.3)
s = Atspi.Text.get_selection(notes, 0); P('drag selection', s.start_offset, s.end_offset)
lab.combo('KEY_LEFTCTRL', 'KEY_X'); time.sleep(0.3); a1 = ax.text(notes); cl = wlpaste('-n')
lab.combo('KEY_LEFTCTRL', 'KEY_Z'); time.sleep(0.3); a2 = ax.text(notes)
lab.combo('KEY_LEFTCTRL', 'KEY_LEFTSHIFT', 'KEY_Z'); time.sleep(0.3); a3 = ax.text(notes)
P('cut/undo/redo', repr(a1), repr(a2), repr(a3), '| wl-paste', repr(cl))
tbl = ax.find(A, 'table'); x, y = ax.center(tbl, C); lab.move(x, y); time.sleep(0.3); lab.screenshot(f'{E}/R04-table-top.png')
first = lambda: Atspi.Component.get_extents([r for _, r in ax.all_nodes(A) if r.get_role_name() == 'table row'][0], Atspi.CoordType.WINDOW).y
y0 = first()
for i in range(6):
    lab.wheel(-3); time.sleep(0.1)
time.sleep(0.6); P('wheel: first row y', y0, '->', first()); lab.screenshot(f'{E}/R04-table-scrolled.png')

P('## R05 (Orca)')
dbg = '/tmp/claude-1000/-home-toor-projects-ui4me-core/d6bab0d7-7bb7-4a6c-b84c-a5812eceb36c/scratchpad/orca-final.out'
if os.path.exists(dbg): os.unlink(dbg)
orca = subprocess.Popen(['orca', '--replace', f'--debug-file={dbg}'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); time.sleep(6)
lab.activate(C); time.sleep(1)
ax.click(name, C); time.sleep(2.5); lab.key('KEY_TAB'); time.sleep(3); lab.key('KEY_DOWN'); time.sleep(3); lab.key('KEY_TAB'); time.sleep(2.5)
pr = ax.find(A, 'combo box', 'Priority'); Atspi.Selection.select_child(pr, 0); time.sleep(2)
P('Selection.select_child(0)', [Atspi.Selection.get_selected_child(pr, i).get_name() for i in range(Atspi.Selection.get_n_selected_children(pr))])
ax.click(pr, C); time.sleep(0.5); lab.key('KEY_ESC'); time.sleep(2.5)
Atspi.EditableText.set_text_contents(name, 'Set via AT-SPI'); time.sleep(0.6); P('EditableText', repr(ax.text(name)))
Atspi.Text.set_selection(name, 0, 0, 3); time.sleep(0.3); s = Atspi.Text.get_selection(name, 0); P('set_selection', s.start_offset, s.end_offset)
cb = ax.find(A, 'check box', 'Include notification flag'); b = cb.get_state_set().contains(Atspi.StateType.CHECKED); Atspi.Action.do_action(cb, 0); time.sleep(0.6); P('checkbox action', b, '->', cb.get_state_set().contains(Atspi.StateType.CHECKED))
sl = ax.find(A, 'slider', 'Progress'); Atspi.Value.set_current_value(sl, 75); time.sleep(0.6); P('slider value', Atspi.Value.get_current_value(sl))
n = len(rows()); Atspi.Action.do_action(ax.find(A, 'button', 'Add row'), 0); time.sleep(2.5); P('Add row action: rows', n, '->', len(rows()))
st = ax.find(A, 'status bar'); P('status attrs', dict(st.get_attributes() or {}))
last = [a for _, a in ax.all_nodes(A) if a.get_role_name() == 'table cell'][-3]; s0 = Atspi.Text.get_selection(name, 0)
ok = Atspi.Component.scroll_to(last, Atspi.ScrollType.ANYWHERE); time.sleep(0.8); s1 = Atspi.Text.get_selection(name, 0)
P('scroll_to last cell', ok, 'y', Atspi.Component.get_extents(last, Atspi.CoordType.WINDOW).y, '| selection unchanged', (s0.start_offset, s0.end_offset) == (s1.start_offset, s1.end_offset))
time.sleep(1); orca.terminate(); orca.wait(5)
speech = [l.split('SPEECH OUTPUT: ', 1)[1].split(" {'established'")[0] for l in open(dbg, errors='replace') if 'SPEECH OUTPUT:' in l]
open(f'{E}/R05-orca-speech.txt', 'w').write('\n'.join(speech) + '\n'); P('orca speech', speech)
subprocess.run(['cp', dbg, f'{E}/R05-orca-debug.out'])
subprocess.run(['venv/bin/python', 'atspi_dump.py', 'UI4ME Desktop Workspace'], stdout=open(f'{E}/R05-atspi-tree.json', 'w'), stderr=subprocess.DEVNULL)

P('## R06 / N08')
before = [x['caption'] for x in lab.windows() if x['res'] == 'org.ui4me.RendererReference']
ax.click(ax.find(A, 'button', 'Open second window'), C); time.sleep(2)
sec = [x['caption'] for x in lab.windows() if x['res'] == 'org.ui4me.RendererReference' and x['caption'] not in before][0]; P('second window', sec)
lab.set_geometry(sec, 1100, 100, 700, 500); time.sleep(1); P('resize', lab.window(sec)['w'], lab.window(sec)['h']); lab.screenshot(f'{E}/R06-second-small.png')
for op, js in (('maximize', 'w.setMaximize(true,true)'), ('unmaximize', 'w.setMaximize(false,false)'), ('minimize', 'w.minimized=true'), ('restore', 'w.minimized=false')):
    lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ var w=ws[i]; if (w.caption.indexOf(%s)>=0){ %s; } } OUT('x')" % (json.dumps(sec), js)); time.sleep(1)
    w = lab.window(sec); P(op, w['w'], w['h'], 'max', w['maximized'], 'min', w['minimized'])
lab.set_geometry(C, 100, 40, 520, 900); time.sleep(1.2); lab.screenshot(f'{E}/R06-main-narrow.png')
ax.click(ax.find(A, 'entry', 'Name'), C); time.sleep(0.3); P('narrow click Name ->', ax.focused(A))
close(sec); time.sleep(1.5); P('after closing second', [x['caption'] for x in lab.windows() if x['res'] == 'org.ui4me.RendererReference'])
for scale in (1.5, 2, 1):
    subprocess.run(['kscreen-doctor', f'output.HDMI-A-1.scale.{scale}'], capture_output=True); time.sleep(3); lab.set_scale(scale)
    lab.set_geometry(C, 20, 20, 1000 if scale == 1 else (880 if scale == 1.5 else 900), 680 if scale != 2 else 500); time.sleep(1.5)
    A = ax.app('UI4ME Desktop Workspace'); w = lab.window(C)
    res = []
    for target in (('entry', 'Notes'), ('entry', 'Name')):
        acc = ax.find(A, *target); e = Atspi.Component.get_extents(acc, Atspi.CoordType.WINDOW)
        lab.click(w['cx'] + (e.x + e.width / 2) / scale, w['cy'] + (e.y + e.height / 2) / scale); time.sleep(0.5); res.append((target[1], ax.focused(A)))
    P(f'scale {scale}: clicks', res); lab.screenshot(f'{E}/N08-scale-{scale}.png')
P('scale restored', subprocess.run(['kscreen-doctor', '-o'], capture_output=True, text=True).stdout.split('Scale:')[1].split('\n')[0].strip()[-1])

P('## R03 (IME + clipboard)')
lab.set_geometry(C, 100, 40, 1100, 900); time.sleep(1)
A = ax.app('UI4ME Desktop Workspace'); notes = ax.find(A, 'entry', 'Notes'); name = ax.find(A, 'entry', 'Name')
ax.click(notes, C); lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.key('KEY_BACKSPACE'); time.sleep(0.3)
im = lambda: subprocess.run(['fcitx5-remote', '-n'], capture_output=True, text=True).stdout.strip()
P('IM', im()); lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.6); P('IM after toggle', im())
lab.type_text('nihao'); time.sleep(1.2); lab.screenshot(f'{E}/R03-preedit-textarea.png'); P('preedit value', repr(ax.text(notes)))
lab.key('KEY_SPACE'); time.sleep(0.8); P('commit', repr(ax.text(notes)))
lab.type_text('shijie'); time.sleep(0.8); lab.key('KEY_ESC'); time.sleep(0.6); P('Escape', repr(ax.text(notes)))
lab.type_text('zhongwen'); time.sleep(0.8); lab.key('KEY_2'); time.sleep(0.8); P('candidate 2', repr(ax.text(notes)))
lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.type_text('ceshi'); time.sleep(0.6); lab.key('KEY_SPACE'); time.sleep(0.8); P('compose over selection', repr(ax.text(notes)))
lab.type_text('hao'); time.sleep(0.6); ax.click(name, C); time.sleep(0.8); P('focus change mid-preedit: Notes', repr(ax.text(notes)), '| Name', repr(ax.text(name)))
lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.6); P('IM back', im())
ax.click(notes, C); lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.combo('KEY_LEFTCTRL', 'KEY_C'); time.sleep(0.5); P('Ctrl+C -> wl-paste', repr(wlpaste('-n')))
p = wlcopy('Žluťoučký 日本 😀 from wl-copy'); time.sleep(0.8)
ax.click(name, C); lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.8); P('wl-copy -> Ctrl+V', repr(ax.text(name))); p.kill()

P('## R08')
A = ax.app('UI4ME Desktop Workspace')
st = lambda: [a.get_name() for _, a in ax.all_nodes(ax.find(ax.app('UI4ME Desktop Workspace'), 'status bar')) if a.get_role_name() == 'label']
dl = lambda: [(x['caption'], x['transient'], x['modal'], x['transientFor']) for x in lab.windows() if x['res'].startswith('org.freedesktop.impl.portal')]
ax.click(ax.find(A, 'button', 'Choose a file'), C); time.sleep(3); P('picker', dl()); lab.screenshot(f'{E}/R08-portal-dialog.png'); lab.key('KEY_ESC'); time.sleep(1.5); P('cancel ->', st(), dl())
p = wlcopy(os.path.expanduser('~/ui4me-test/žluťoučký 日本.txt')); time.sleep(0.5)
ax.click(ax.find(A, 'button', 'Choose a file'), C); time.sleep(3); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.8); lab.key('KEY_ENTER'); time.sleep(2.5); p.kill(); P('select ->', st(), dl())
ax.click(ax.find(A, 'button', 'About this workspace'), C); time.sleep(1.2); lab.screenshot(f'{E}/R08-html-dialog.png')
A = ax.app('UI4ME Desktop Workspace'); P('HTML dialog focus', ax.focused(A), [(a.get_role_name()) for _, a in ax.all_nodes(A) if a.get_role_name() == 'dialog'])
lab.key('KEY_ESC'); time.sleep(0.8); P('Escape -> focus', ax.focused(A))
ax.click(ax.find(A, 'button', 'Quick help'), C); time.sleep(1.2); lab.screenshot(f'{E}/R08-popover.png'); P('popover', [a.get_name()[:40] for _, a in ax.all_nodes(A) if a.get_role_name() == 'label' and 'name' in (a.get_name() or '').lower()][:1])
lab.key('KEY_ESC'); time.sleep(0.8); P('Escape popover -> focus', ax.focused(A))
hp = lab.window(C)['pid']
ax.click(ax.find(A, 'button', 'Choose a file'), C); time.sleep(3); P('picker open', dl())
close(C); time.sleep(3)
P('parent close -> picker', dl(), 'app windows', [x['caption'] for x in lab.windows() if x['res'] == 'org.ui4me.RendererReference'], 'host alive', subprocess.run(['kill', '-0', str(hp)], capture_output=True).returncode == 0)
P('DONE')
