"""Final-binary completion suite C01-C12 on the installed C-lab."""
import lab, ax, time, json, subprocess, os, re, threading, base64, zlib, struct
import clabctl as c, hostinfo as hi
from gi.repository import Atspi
from PIL import Image, ImageChops
R = '/home/toor/projects/ui4me-core'
E = R + '/verification/core-1.0-linux-final/evidence'
HOST = os.path.expanduser('~/.cargo/bin/ui4me-host')
H = os.path.expanduser('~')
S = '/tmp/claude-1000/-home-toor-projects-ui4me-core/d6bab0d7-7bb7-4a6c-b84c-a5812eceb36c/scratchpad'
C = 'C-lab'
def P(*a): print(*a, flush=True)
def js(cap, code, wait=0.6): lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ var w=ws[i]; if (w.caption.indexOf(%s)>=0){ %s; } } OUT('x')" % (json.dumps(cap), code), wait=wait)
_n = [0]
def restart():
    if lab.window(C): js(C, 'w.closeWindow()'); time.sleep(3)
    p = os.path.expanduser('~/.local/state/ui4me-clab/events.jsonl')
    if os.path.exists(p): os.unlink(p)
    _n[0] += 1
    subprocess.Popen([HOST, 'run', '--app', os.path.expanduser('~/.local/share/ui4me-core1-clab/app.toml')], cwd=H, stdout=open(f'{E}/C-lab-host-{_n[0]}.log', 'w'), stderr=subprocess.STDOUT)
    lab.wait_window(C); time.sleep(2); lab.set_geometry(C, 300, 40, 1002, 789); time.sleep(1)
    P('  [lab started:', [e['data'] for e in c.events(0, 'step')], ']')
val = lambda e: c.op('get_value', element=e).get('result', {}).get('value')
sel = lambda e: c.op('get_control_selection', element=e).get('result')
im = lambda: subprocess.run(['fcitx5-remote', '-n'], capture_output=True, text=True).stdout.strip()
def wlcopy(t): return subprocess.Popen(['wl-copy', t], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

restart()
P('## C01')
def geo(): w = lab.window(C); return (round(w['cw']), round(w['ch']))
def br(dx, dy):
    w = lab.window(C); x, y = w['x'] + w['w'] + 3, w['y'] + w['h'] + 3
    lab.drag(x, y, x + dx, y + dy, steps=30, dt=0.015); time.sleep(0.7); return geo()
ws = lambda **k: c.call('window.set', window='main', **k)
get = lambda: c.call('window.get', window='main')['result']
P('initial', geo(), get()['requested']['size_constraints'])
P('drag -800,-600 ->', br(-800, -600)); lab.set_geometry(C, 20, 20, 1002, 789); time.sleep(0.8); P('drag +700,+300 ->', br(700, 300))
ws(min_width=600, min_height=400, max_width=900, max_height=700); time.sleep(0.8); P('limits 600x400..900x700 ->', geo())
lab.set_geometry(C, 300, 40, 902, 729); time.sleep(0.6); P('drag -600,-500 ->', br(-600, -500), '| +600,+500 ->', br(600, 500))
ws(min_width=None, min_height=None); P('min cleared: drag -800,-600 ->', br(-800, -600))
ws(max_width=None, max_height=None); lab.set_geometry(C, 20, 20, 700, 500); time.sleep(0.6); P('max cleared: drag +1000,+400 ->', br(1000, 400))
before = get()['requested']['size_constraints']
for bad in ({'min_width': 500}, {'min_width': 0, 'min_height': 0}, {'min_width': 900, 'min_height': 900, 'max_width': 800, 'max_height': 800}, {'max_width': 100000, 'max_height': 100000}):
    r = ws(**bad); P('invalid', bad, '->', r.get('code'), (r.get('error') or '')[:70])
P('unchanged after invalid', get()['requested']['size_constraints'] == before)
lab.set_geometry(C, 300, 40, 1000, 760); time.sleep(0.6)
for op, code in (('maximize', 'w.setMaximize(true,true)'), ('restore', 'w.setMaximize(false,false)'), ('fullscreen', 'w.fullScreen=true'), ('leave', 'w.fullScreen=false')):
    js(C, code); time.sleep(1); o = get()['observed']; P(op, {k: o[k] for k in ('maximized', 'fullscreen', 'focused', 'minimized', 'position', 'visible')})
lab.activate('Konsole'); time.sleep(0.6); P('Konsole focused -> observed focused', get()['observed']['focused'])
for k in ({'visible': False}, {'focus': True}, {'x': 10, 'y': 10}, {'minimized': False}):
    r = ws(**k); P('unsupported', k, r.get('code'), (r.get('error') or '')[:80])
r = c.call('window.create', title='hidden', width=300, height=200, visible=False); P('hidden create', r.get('code'), (r.get('error') or '')[:80])
for kind in ('informational', 'critical'): P('attention', kind, c.call('window.request_attention', window='main', kind=kind).get('result'))
P('attention cancel', c.call('window.request_attention', window='main', kind=None).get('error')); time.sleep(1); lab.screenshot(f'{E}/C01-attention-taskbar.png')
ws(min_width=480, min_height=360, max_width=1400, max_height=1000); lab.set_geometry(C, 300, 40, 1002, 789); time.sleep(0.6)

P('## C02')
css = "@media (prefers-color-scheme: dark){#gridcap{color:rgb(200,1,1)}} @media (prefers-color-scheme: light){#gridcap{color:rgb(1,1,200)}} #gridnote{transition-duration:1s} @media (prefers-reduced-motion: reduce){#gridnote{transition-duration:0s}}"
c.op('add_stylesheet', css=css)
def probe(tag):
    env = c.op('get_environment').get('result'); pref = c.call('desktop.preferences').get('result', {})
    cs = c.op('get_computed_style', element='gridcap', properties=['color'])['result']['properties']['color']['value']
    tr = c.op('get_computed_style', element='gridnote', properties=['transition-duration'])['result']['properties']['transition-duration']['value']
    P(f"{tag}: prefs cs={pref.get('color_scheme')} rm={pref.get('reduced_motion')} hc={pref.get('high_contrast')} src={pref.get('source')} | env cs={env['color_scheme']}({env['color_scheme_source']}) rm={env['reduced_motion']}({env['reduced_motion_source']}) hc={env['high_contrast']} | gridcap={cs} gridnote={tr}")
scheme = lambda n: (subprocess.run(['plasma-apply-colorscheme', n], capture_output=True), time.sleep(3))
probe('initial'); t = time.time(); scheme('BreezeLight'); probe('OS BreezeLight'); P('  notifications', len(c.events(t, 'desktop.preferences_changed')))
scheme('BreezeDark'); probe('OS BreezeDark')
c.op('set_environment', color_scheme='dark'); scheme('BreezeLight'); probe('override dark + OS light')
c.op('set_environment', color_scheme='system'); time.sleep(1); probe('override system')
c.op('set_environment', color_scheme='light'); scheme('BreezeDark'); probe('override light + OS dark')
c.op('set_environment', color_scheme='system'); time.sleep(1); probe('override system')
c.op('set_environment', reduced_motion='reduce'); probe('rm reduce'); c.op('set_environment', reduced_motion='no-preference'); probe('rm no-preference'); c.op('set_environment', reduced_motion='system'); probe('rm system')
subprocess.run(['gsettings', 'set', 'org.gnome.desktop.a11y.interface', 'high-contrast', 'true']); time.sleep(3); probe('high-contrast true')
subprocess.run(['gsettings', 'set', 'org.gnome.desktop.a11y.interface', 'high-contrast', 'false']); time.sleep(3); probe('high-contrast false')
subprocess.run(['systemctl', '--user', 'restart', 'xdg-desktop-portal.service']); time.sleep(4); scheme('BreezeLight'); probe('after portal restart, OS light'); scheme('BreezeDark'); probe('restored dark')
hp = lab.window(C)['pid']; names = [l.split()[0] for l in subprocess.run(['busctl', '--user', 'list', '--no-legend'], capture_output=True, text=True).stdout.splitlines() if len(l.split()) > 1 and l.split()[1] == str(hp)]
mon = subprocess.run(['timeout', '30', 'dbus-monitor', '--session'], capture_output=True, text=True).stdout
P('idle 30 s D-Bus messages from host connections', {n: mon.count(f'sender={n}') for n in names})

restart()
P('## C04')
P('t1', repr(val('t1')))
c.op('set_control_selection', element='t1', start=1, end=4, direction='forward'); P('forward', sel('t1'))
c.op('set_control_selection', element='t1', start=2, end=4, direction='backward'); P('backward', sel('t1'))
r = c.op('set_control_selection', element='t1', start=3, end=5); P('split surrogate', r.get('code'), '| unchanged', sel('t1'))
r = c.op('replace_control_range', element='t1', replacement='X', start=3, end=4); P('replace split', r.get('code'), repr(val('t1')))
c.op('replace_control_range', element='t1', replacement='ZZ', start=2, end=4, selection_mode='select'); P('replace emoji', repr(val('t1')), sel('t1'))
c.op('edit_command', command='undo', element='t1'); P('undo', repr(val('t1'))); c.op('edit_command', command='redo', element='t1'); P('redo', repr(val('t1'))); c.op('edit_command', command='undo', element='t1'); P('undo', repr(val('t1')))
c.click_el(lab, 't1'); lab.key('KEY_HOME'); offs = []
for i in range(7):
    lab.key('KEY_RIGHT'); time.sleep(0.15); offs.append(sel('t1')['start'])
P('caret offsets', offs)
lab.key('KEY_LEFT'); lab.key('KEY_BACKSPACE'); time.sleep(0.3); P('Backspace at 8', repr(val('t1'))); lab.combo('KEY_LEFTCTRL', 'KEY_Z'); time.sleep(0.3)
c.op('set_control_selection', element='t1', start=2, end=2); lab.key('KEY_DELETE'); time.sleep(0.3); P('Delete at 2', repr(val('t1'))); lab.combo('KEY_LEFTCTRL', 'KEY_Z'); time.sleep(0.3); P('restored', repr(val('t1')))
for el in ('ro', 'cancel-field'):
    c.click_el(lab, el); b = val(el); lab.key('KEY_END'); lab.type_text('zz'); time.sleep(0.4); P('typing', el, b, '->', val(el))
p = wlcopy('pasted'); time.sleep(0.5); c.click_el(lab, 'cancel-field'); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.5); P('paste cancel-field', val('cancel-field')); p.kill()
P('programmatic replace cancel-field', c.op('replace_control_range', element='cancel-field', replacement='prog', start=0, end=0).get('result'), val('cancel-field'))
P('beforeinput cancel-field', [(e.get('input_type'), e.get('default_prevented')) for e in c.doc_events(0, {'beforeinput'}) if e.get('element') == 'cancel-field'])
for el in ('t2', 'mail', 'pw'): c.op('set_value', element=el, value='')
def ime(el, focus='pointer'):
    if focus == 'pointer': c.click_el(lab, el)
    else: c.op('focus', element=el); time.sleep(0.4)
    lab.key('KEY_END'); a = im(); lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.5); b = im()
    lab.type_text('nihao'); time.sleep(0.8); pre = val(el); lab.key('KEY_SPACE'); time.sleep(0.6); post = val(el)
    lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.4); return f'IM {a}->{b} preedit={pre!r} commit={post!r}'
for el in ('t2', 'mail', 'ta', 'pw', 'ro'): P('IME', el, ime(el))
c.op('set_value', element='t2', value=''); P('IME t2 programmatic focus', ime('t2', 'program')); lab.screenshot(f'{E}/C04-after-ime.png')
c.op('set_value', element='t2', value=''); c.op('set_value', element='mail', value='')
c.click_el(lab, 't2'); lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.4); lab.type_text('zhong'); time.sleep(0.6); lab.screenshot(f'{E}/C04-preedit-t2.png')
c.click_el(lab, 'mail'); time.sleep(0.6); P('focus change mid-composition: t2', repr(val('t2')), 'mail', repr(val('mail')))
lab.type_text('wen'); time.sleep(0.5); lab.key('KEY_SPACE'); time.sleep(0.5); P('  next composition in mail', repr(val('mail')), '| t2', repr(val('t2')))
c.click_el(lab, 't2'); lab.key('KEY_END'); lab.type_text('shi'); time.sleep(0.5); lab.key('KEY_ESC'); time.sleep(0.4); P('Escape in t2', repr(val('t2')))
lab.combo('KEY_LEFTCTRL', 'KEY_SPACE'); time.sleep(0.3); P('IM back', im())

P('## C05')
P('form_data', c.op('get_form_data', element='form-a')['result'])
P('form_data sub2', c.op('get_form_data', element='form-a', submitter='sub2')['result']['submitter'])
t = time.time(); P('request_submit sub1', c.op('request_submit', element='form-a', submitter='sub1')['result']); time.sleep(0.4); P('  events', [(e['submitter'], e['default_prevented']) for e in c.doc_events(t, {'submit'})])
t = time.time(); c.click_el(lab, 'sub2'); time.sleep(0.5); P('click Send', [(e['submitter'], e['default_prevented']) for e in c.doc_events(t, {'submit'})])
t = time.time(); c.click_el(lab, 'req'); lab.key('KEY_END'); lab.key('KEY_ENTER'); time.sleep(0.5); P('Enter', [(e['submitter'], e['default_prevented']) for e in c.doc_events(t, {'submit'})])
t = time.time(); c.op('focus', element='sub1'); lab.key('KEY_SPACE'); time.sleep(0.5); P('Space on Save', [(e['submitter']) for e in c.doc_events(t, {'submit'})])
c.op('set_value', element='req', value=''); P('valueMissing', c.op('get_validity', element='req')['result']['flags']['valueMissing'])
t = time.time(); P('submit invalid', c.op('request_submit', element='form-a', submitter='sub1')['result'], 'submits', len(c.doc_events(t, {'submit'})), 'invalid', len(c.doc_events(t, {'invalid'})))
c.op('set_value', element='req', value='filled'); c.op('set_custom_validity', element='req', message='Custom problem')
t = time.time(); P('custom invalid', c.op('request_submit', element='form-a')['result'], 'submits', len(c.doc_events(t, {'submit'})))
c.op('set_custom_validity', element='req', message=''); t = time.time(); P('cleared', c.op('request_submit', element='form-a')['result'], 'submits', len(c.doc_events(t, {'submit'})))
c.click_el(lab, 'req'); lab.key('KEY_END'); lab.type_text('XYZ'); c.op('set_checked', element='cb', checked=True); c.op('set_selected_values', element='pick', values=['Apple'])
P('before reset', val('req'), c.op('get_checked', element='cb')['result'], val('pick')); c.click_el(lab, 'rst'); time.sleep(0.4)
P('after reset', val('req'), c.op('get_checked', element='cb')['result'], val('pick')); c.click_el(lab, 'req'); lab.combo('KEY_LEFTCTRL', 'KEY_Z'); time.sleep(0.3); P('Ctrl+Z after reset', val('req'))
def pick(el, paths, cancel=False):
    c.click_el(lab, el); time.sleep(3); dl = [x['caption'] for x in lab.windows() if x['res'].startswith('org.freedesktop.impl.portal')]
    if cancel: lab.key('KEY_ESC')
    else:
        if len(paths) > 1:
            q = wlcopy(os.path.dirname(paths[0]) + '/'); time.sleep(0.4); lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.4); lab.key('KEY_ENTER'); time.sleep(1.2); q.kill()
            txt = ' '.join('"%s"' % os.path.basename(p) for p in paths)
        else: txt = paths[0]
        q = wlcopy(txt); time.sleep(0.4); lab.combo('KEY_LEFTCTRL', 'KEY_A'); lab.combo('KEY_LEFTCTRL', 'KEY_V'); time.sleep(0.5); lab.key('KEY_ENTER'); q.kill()
    time.sleep(2); return dl
P('file1', pick('file1', [H + '/ui4me-test/žluťoučký 日本.txt']), c.op('get_files', element='file1')['result'])
P('file2', pick('file2', [H + '/ui4me-test/multi/one.txt', H + '/ui4me-test/multi/dva ž.txt']), c.op('get_files', element='file2')['result'])
lab.screenshot(f'{E}/C05-files-selected.png'); P('form_data files', [x for x in c.op('get_form_data', element='form-a')['result']['entries'] if x['kind'] == 'file'])
P('file1 cancel', pick('file1', [], True), c.op('get_files', element='file1')['result'])
c.click_el(lab, 'rst'); time.sleep(0.5); P('after reset', c.op('get_files', element='file1')['result'], c.op('get_files', element='file2')['result']); lab.screenshot(f'{E}/C05-after-reset.png')

P('## C07')
c.op('scroll_into_view', element='s-c03', block='start'); time.sleep(1.5)
w = lab.window(C)
def at(z, fx=0.5, fy=0.5):
    r = c.op('get_rect', element=z)['result']['rect']; return w['cx'] + r['x'] + r['width'] * fx, w['cy'] + r['y'] + r['height'] * fy
ox, oy = at('ev-outer', 0.02); ix, iy = at('ev-inner'); lab.move(ox - 40, oy - 60); time.sleep(0.4); t = time.time()
for x, y in ((ox + 5, oy), (ix, iy), (ox + 5, oy), (ox - 60, oy - 60)): lab.move(x, y); time.sleep(0.4)
P('boundary', [(e['type'], e.get('element'), e.get('related_element')) for e in c.doc_events(t, {'mouseover', 'mouseout', 'mouseenter', 'mouseleave'}) if e.get('element') in ('ev-outer', 'ev-inner')])
t = time.time(); lab.click(ix, iy, name='right'); time.sleep(0.5); P('right', [(e['type'], e.get('button')) for e in c.doc_events(t, {'contextmenu', 'auxclick', 'mousedown', 'mouseup', 'click'})])
t = time.time(); lab.click(ix, iy, name='middle'); time.sleep(0.5); P('middle', [(e['type'], e.get('button')) for e in c.doc_events(t, {'auxclick', 'click', 'mousedown', 'mouseup'})])
t = time.time(); lab.click(*at('ctx-cancel'), name='right'); time.sleep(0.5); P('ctx-cancel', [(e['type'], e.get('default_prevented')) for e in c.doc_events(t, {'contextmenu'})])
t = time.time(); lab.dblclick(*at('dbl')); time.sleep(0.5); P('dbl', [(e['type'], e.get('detail')) for e in c.doc_events(t, {'click', 'dblclick'})])
c.op('set_value', element='t2', value=''); c.click_el(lab, 't2'); t = time.time(); lab.type_text('ab'); time.sleep(0.4)
P('typing events', [(e['type'], e.get('input_type'), e.get('data')) for e in c.doc_events(t, {'beforeinput', 'input', 'selectionchange'})])
c.op('scroll_into_view', element='s-c03', block='start'); time.sleep(1.5); w = lab.window(C); c.op('set_value', element='t2', value='')
x, y = at('block-python'); t = time.time(); lab.click(x, y); tb = time.time(); time.sleep(0.6)
lab.click(*at('t2')); lab.type_text('typed while blocked'); time.sleep(0.3); v = val('t2')
lab.move(*at('ev-outer')); [(lab.wheel(-3), time.sleep(0.1)) for _ in range(3)]; time.sleep(0.4)
P('%.1f s into block: value %r, t2 rect y %.0f, block log %s' % (time.time() - tb, v, c.op('get_rect', element='t2')['result']['rect']['y'], [e['data'] for e in c.events(t, 'python-block')]))
time.sleep(9); P('block log', [(k, round(vv - tb, 2)) for b in [e['data'] for e in c.events(t, 'python-block')] for k, vv in b.items()])

P('## C08')
A = ax.app('UI4ME Core Lab'); P('app name', A.get_name())
grid = ax.find(A, pred=lambda a: a.get_role_name() == 'table' and 'Inventory' in (a.get_name() or '')); P('grid', grid.get_name(), dict(grid.get_attributes()))
P('cells', [(a.get_role_name(), a.get_name(), {k: v for k, v in dict(a.get_attributes()).items() if k in ('rowindex', 'colindex')}) for _, a in ax.all_nodes(grid) if a.get_role_name() in ('table cell', 'column header')])
rels = []
for _, a in ax.all_nodes(A):
    try:
        for rr in a.get_relation_set(): rels.append((a.get_role_name(), a.get_name(), rr.get_relation_type().value_nick))
    except Exception: pass
P('relations', rels)
tree = ax.find(A, 'tree', 'Folders')
P('tree', [(a.get_name()[:10], {k: v for k, v in dict(a.get_attributes()).items() if k in ('level', 'posinset', 'setsize')}, a.get_state_set().contains(Atspi.StateType.SELECTED), a.get_state_set().contains(Atspi.StateType.EXPANDED)) for _, a in ax.all_nodes(tree) if a.get_role_name() in ('tree item', 'list item')])
live = ax.find(A, pred=lambda a: dict(a.get_attributes() or {}).get('id') == 'live'); P('live', dict(live.get_attributes()))
c.op('set_attribute', element='live', name='aria-busy', value='true'); time.sleep(0.5); P('busy', live.get_state_set().contains(Atspi.StateType.BUSY)); c.op('set_attribute', element='live', name='aria-busy', value='false')
c.op('scroll_into_view', element='s-c08', block='start'); time.sleep(1)
par = ax.find(A, pred=lambda a: dict(a.get_attributes() or {}).get('id') == 'far'); leaf = ax.find(A, pred=lambda a: 'Far away' in (a.get_name() or '') and a.get_role_name() == 'label')
for tgt, lbl in ((par, 'paragraph'), (leaf, 'static-text leaf')):
    c.op('scroll_to', element='scroller', x=0, y=0); time.sleep(0.5); c.op('set_control_selection', element='t1', start=1, end=3); s0 = sel('t1')
    ok = Atspi.Component.scroll_to(tgt, Atspi.ScrollType.TOP_EDGE); time.sleep(0.8); P(f'scroll_to({lbl})', ok, 'scroller y', c.op('get_scroll', element='scroller')['result']['y'], 'selection unchanged', sel('t1')['start'] == s0['start'])
ta = ax.find(A, 'entry', pred=lambda a: a.get_text_iface() is not None and 'line one' in ax.text(a))
c.op('set_control_selection', element='ta', start=2, end=5); s0 = sel('ta'); c.op('scroll_into_view', element='s-c09', block='end'); time.sleep(0.8)
y0 = c.op('get_rect', element='ta')['result']['rect']['y']; ok = Atspi.Text.scroll_substring_to(ta, 9, 12, Atspi.ScrollType.ANYWHERE); time.sleep(0.8)
P('text-run scroll', ok, 'y', y0, '->', c.op('get_rect', element='ta')['result']['rect']['y'], 'selection unchanged', sel('ta')['start'] == s0['start'])
combo = ax.find(A, 'combo box', 'Fruit'); Atspi.Selection.select_child(combo, 2); time.sleep(0.5); P('combo select_child(2)', val('combo'))
cell = [a for _, a in ax.all_nodes(grid) if a.get_role_name() == 'table cell'][0]
c.req({'kind': 'load', 'page': 'a.html'}); time.sleep(1); P('old cell after replacement defunct', cell.get_state_set().contains(Atspi.StateType.DEFUNCT))

P('## C03 (replaces documents)')
c.req({'kind': 'load', 'page': 'fixture.html'}); time.sleep(1)
q = c.op('query_nodes', selector='ul.anon > li')['result']; refs = [n['node'] for n in q['nodes']]; P('query_nodes', q)
P('set_text beta', c.op('node_batch', ops=[{'op': 'set_text', 'node': refs[1], 'text': 'BETA (mutated)'}]).get('ok'))
P('valid+stale batch', c.op('node_batch', ops=[{'op': 'set_text', 'node': refs[0], 'text': 'SHOULD ROLL BACK'}, {'op': 'set_text', 'node': 'node:9:99999:1', 'text': 'x'}]).get('error'))
lab.screenshot(f'{E}/C03-after-mutation.png')
P('remove gamma', c.op('node_batch', ops=[{'op': 'remove_element', 'node': refs[2]}]).get('ok'), '| insert replacement', c.op('node_batch', ops=[{'op': 'insert_html', 'node': refs[1], 'position': 'afterend', 'html': '<li id="repl">replacement</li>'}]).get('ok'))
r = c.op('node_batch', ops=[{'op': 'set_text', 'node': refs[2], 'text': 'stale'}]); P('reuse removed handle', r.get('code'), r.get('error'), '| replacement', c.op('get_text', element='repl')['result'])
c.op('scroll_into_view', element='s-c03', block='start'); time.sleep(1); A = ax.app('UI4ME Core Lab')
t = time.time(); ax.click(ax.find(A, 'button', 'Nested RPC'), C); time.sleep(1); P('nested RPC', c.op('get_text', element='nested-out')['result'])
t = time.time(); ax.click(ax.find(A, 'button', 'Remove myself'), C); time.sleep(2.5)
P('self-remove', [e['data'] for e in c.events(t, 'self-remove')], 'clicks', len([e for e in c.doc_events(t, {'click'}) if e.get('element') == 'self-remove']), 'recreated', [a.get_name() for _, a in ax.all_nodes(ax.app('UI4ME Core Lab')) if a.get_name() == 'Recreated'])
lab.screenshot(f'{E}/C03-after-removal.png')
ids = c.req({'kind': 'ids'})['result']; old = ids['main']['document_id']; keep = refs[1]
c.req({'kind': 'load', 'page': 'a.html'})
r = c.req({'kind': 'rawdoc', 'params': {'window_id': ids['main']['window_id'], 'document_id': old, 'command': {'op': 'get_text', 'element': 'nested-out'}}}); P('old document id', r.get('code'), r.get('error'))
r = c.op('node_batch', ops=[{'op': 'set_text', 'node': keep, 'text': 'x'}]); P('old node on new document', r.get('code'), r.get('error'))
c.req({'kind': 'load', 'page': 'fixture.html'}); time.sleep(1)

P('## C09')
def png(w, h, rgba):
    raw = b''.join(b'\0' + bytes(rgba) * w for _ in range(h))
    ch = lambda t, d: struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + ch(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + ch(b'IDAT', zlib.compress(raw)) + ch(b'IEND', b'')
ent = lambda p, d, m='image/png': {'path': p, 'data_url': 'data:%s;base64,%s' % (m, base64.b64encode(d).decode())}
reg = lambda n, e: c.op('register_resource_bundle', name=n, entries=e)
c.op('scroll_into_view', element='s-c09', block='end'); time.sleep(0.8)
def color():
    r = c.op('get_rect', element='res-img')['result']['rect']; w = lab.window(C)
    return Image.open(lab.screenshot(S + '/ev/c09.png')).convert('RGB').getpixel((int(w['cx'] + r['x'] + r['width'] / 2), int(w['cy'] + r['y'] + r['height'] / 2)))
P('red', reg('bundle', [ent('a.png', png(48, 48, (255, 0, 0, 255)))]).get('ok'), color()); lab.screenshot(f'{E}/C09-red.png')
P('blue', reg('bundle', [ent('a.png', png(48, 48, (0, 0, 255, 255)))])['result'], color()); lab.screenshot(f'{E}/C09-blue.png')
for lbl, es in (('../x', [ent('../x.png', b'x')]), ('/x', [ent('/x.png', b'x')]), ('257', [ent(f'e{i}.png', b'x') for i in range(257)]), ('>1MiB', [ent('b.bin', b'\0' * (1024 * 1024 + 1), 'application/octet-stream')])):
    r = reg('bundle', es); P('invalid', lbl, r.get('code'), (r.get('error') or '')[:70])
time.sleep(0.6); P('old bundle still shown', color())
ok = 0; last = None
for i in range(20):
    r = reg(f'agg{i}', [ent('b.bin', bytes([i]) * (1000 * 1024), 'application/octet-stream')])
    if r.get('ok'): ok += 1
    else: last = r.get('error'); break
P('aggregate accepted', ok, last)
for i in range(ok): c.op('unregister_resource_bundle', name=f'agg{i}')
n = 0; last = None
for i in range(70):
    r = reg(f'n{i}', [ent('t.txt', b'x', 'text/plain')])
    if r.get('ok'): n += 1
    else: last = r.get('error'); break
P('count accepted', n, last)
for i in range(n): c.op('unregister_resource_bundle', name=f'n{i}')
P('stats', json.dumps(c.op('get_resource_stats')['result']))
P('purge displayed', c.op('purge_unused_resources')['result']['purged'], color())
c.op('remove_element', element='res-img'); time.sleep(0.5); P('purge after removal', json.dumps(c.op('purge_unused_resources')['result']))
P('unregister', c.op('unregister_resource_bundle', name='bundle')['result'], json.dumps(c.op('get_resource_stats')['result']['bundles']))
reg('broken', [ent('bad.png', b'\x89PNG\r\n\x1a\nGARBAGE')]); c.op('insert_html', element='s-c09', position='beforeend', html='<img id="badimg" src="ui4me-res://broken/bad.png">'); time.sleep(1)
P('decode failure', sorted(set(re.findall(r'"code": "([A-Z_]+)"', json.dumps(c.op('get_diagnostics')['result'])))), c.op('has_element', element='badimg')['result'])

P('## C10')
ids_ = ["fb-min", "fb-max", "bal", "bal-end", "bw", "bw-end", "sel", "rz", "sh", "liga-off", "liga-on", "png", "png-end"]
os.makedirs(f'{E}/c10', exist_ok=True)
ch = json.loads(subprocess.check_output([os.path.expanduser('~/.local/share/ui4me-qa-venv/bin/python'), 'c10_chromium.py', f'{E}/c10'], text=True))
json.dump(ch, open(f'{E}/c10/chromium.json', 'w'), indent=1)
c.req({'kind': 'load', 'page': 'c10.html'}); c.call('window.set', window='main', min_width=None, min_height=None, max_width=None, max_height=None)
ui = {}; worst = 0
for wd in (360, 640, 900):
    c.call('window.set', window='main', width=wd, height=900); time.sleep(1.5); ui[wd] = {i: c.op('get_rect', element=i)['result']['rect'] for i in ids_}
    win = lab.window(C); Image.open(lab.screenshot(f'{E}/c10/ui4me-{wd}-screen.png')).crop((int(win['cx']), int(win['cy']), int(win['cx']) + wd, int(win['cy']) + min(900, int(win['ch'])))).save(f'{E}/c10/ui4me-{wd}.png')
    for i in ids_: worst = max(worst, max(abs(ui[wd][i][k] - ch[str(wd)][i][k]) for k in ('x', 'y', 'width', 'height')))
json.dump(ui, open(f'{E}/c10/ui4me.json', 'w'), indent=1); P('Chromium', ch['version'], '| max |geometry delta| over 13 markers x 3 widths:', worst, 'px')
u = Image.open(f'{E}/c10/ui4me-640.png').convert('RGB'); cc = Image.open(f'{E}/c10/chromium-640.png').convert('RGB')
reg_ = lambda im, x, y, w, h: im.crop((x, y, x + w, y + h))
for nm, b in {'png glyph': (8, 490, 32, 32), 'inset shadow': (8, 318, 120, 50), 'liga-off': (8, 371, 243, 55)}.items():
    P(nm, 'pixels differing >48:', sum(1 for p in ImageChops.difference(reg_(u, *b), reg_(cc, *b)).get_flattened_data() if max(p) > 48), 'of', b[2] * b[3])
P('PNG glyph red pixels', sum(1 for p in reg_(u, 8, 490, 32, 32).get_flattened_data() if p[0] > 180 and p[1] < 80 and p[2] < 80), '| liga default vs override differ', ImageChops.difference(reg_(u, 8, 371, 243, 55), reg_(u, 8, 427, 243, 55)).getbbox() is not None)
c.call('window.set', window='main', width=1000, height=760, min_width=480, min_height=360, max_width=1400, max_height=1000); c.req({'kind': 'load', 'page': 'fixture.html'}); time.sleep(1)

P('## C12 animation')
lab.set_geometry(C, 300, 40, 1002, 789); hp = lab.window(C)['pid']; lab.activate('Konsole'); time.sleep(2)
def m(tag, secs=20):
    c0, t0 = hi.ticks(hp), time.time(); time.sleep(secs); P(f'{tag}: host {(hi.ticks(hp)-c0)/(time.time()-t0):.2f}%')
m('baseline'); s = c.op('add_stylesheet', css='@keyframes spin{to{transform:rotate(360deg)}} .spin{animation:spin 2s linear infinite}')['result']['sheet']
c.op('add_class', element='dbl', name='spin'); m('animation'); c.op('remove_stylesheet', sheet=s); m('sheet removed')
s = c.op('add_stylesheet', css='@keyframes spin{to{transform:rotate(360deg)}} .spin{animation:spin 2s linear infinite}')['result']['sheet']; m('re-added', 10)
c.op('replace_stylesheet', sheet=s, css='.spin{color:red}'); m('replaced static')
c.op('replace_stylesheet', sheet=s, css='@keyframes spin{to{transform:rotate(360deg)}} .spin{animation:spin 2s linear infinite}'); time.sleep(3); c.op('remove_class', element='dbl', name='spin'); m('class removed')
P('DONE')
