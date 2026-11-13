"""Final-binary completion suite part 2: C06 drops, C11 external open/reveal."""
import lab, ax, time, json, subprocess, os, threading
import clabctl as c
from gi.repository import Atspi
R = '/home/toor/projects/ui4me-core'
E = R + '/verification/core-1.0-linux-final/evidence'
HOST = os.path.expanduser('~/.cargo/bin/ui4me-host')
H = os.path.expanduser('~')
C = 'C-lab'
def P(*a): print(*a, flush=True)
def js(cap, code, wait=0.6): lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ var w=ws[i]; if (w.caption.indexOf(%s)>=0){ %s; } } OUT('x')" % (json.dumps(cap), code), wait=wait)
def restart(n):
    if lab.window(C): js(C, 'w.closeWindow()'); time.sleep(3)
    p = os.path.expanduser('~/.local/state/ui4me-clab/events.jsonl')
    if os.path.exists(p): os.unlink(p)
    subprocess.Popen([HOST, 'run', '--app', os.path.expanduser('~/.local/share/ui4me-core1-clab/app.toml')], cwd=H, stdout=open(f'{E}/C-lab-host-p2-{n}.log', 'w'), stderr=subprocess.STDOUT)
    lab.wait_window(C); time.sleep(2)

P('## C06')
restart(1)
subprocess.Popen(['dolphin', '--new-window', H + '/ui4me-dnd'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
subprocess.Popen(['kwrite', H + '/ui4me-dnd/source.txt'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); time.sleep(5)
lab.set_geometry(C, 1040, 40, 1002, 789); lab.set_geometry('ui4me-dnd — Dolphin', 10, 40, 700, 450)
K = [w for w in lab.windows() if 'source.txt' in w['caption']][0]['caption']; lab.set_geometry(K, 10, 520, 700, 400); time.sleep(1)
def zone(z, top=False, cap=C, win='main'):
    r = c.op('get_rect', window=win, element=z)['result']['rect']; w = lab.window(cap); return w['cx'] + r['x'] + 40, w['cy'] + r['y'] + (12 if top else r['height'] / 2)
def src_file():
    d = Atspi.get_desktop(0)
    for i in range(d.get_child_count()):
        a = d.get_child_at_index(i)
        if a.get_name() == 'dolphin':
            it = ax.find(a, 'list item', 'žluťoučký 日本.txt')
            if it:
                e = Atspi.Component.get_extents(it, Atspi.CoordType.WINDOW); w = lab.window('ui4me-dnd — Dolphin'); return w['cx'] + e.x + 60, w['cy'] + e.y + e.height / 2
def src_text():
    lab.activate(K); time.sleep(0.3); lab.combo('KEY_LEFTCTRL', 'KEY_A'); time.sleep(0.2)
    d = Atspi.get_desktop(0)
    for i in range(d.get_child_count()):
        a = d.get_child_at_index(i)
        if a.get_name() in ('kwrite', 'KWrite'):
            ed = ax.find(a, pred=lambda x: x.get_text_iface() is not None and 'drag this' in Atspi.Text.get_text(x, 0, -1))
            if ed:
                r = Atspi.Text.get_character_extents(ed, 5, Atspi.CoordType.WINDOW); w = lab.window(K); return w['cx'] + r.x, w['cy'] + r.y + r.height / 2
def summ(t):
    o = []
    for e in c.doc_events(t, {'drop', 'drag_feedback'}):
        if e['type'] == 'drop': o.append(('DROP', e.get('element'), str(e.get('paths') or e.get('text'))[:70]))
        elif e.get('element'): o.append(('fb', e.get('element'), e.get('accepted'), e.get('kind')))
    return o
for z, top in (('dz-files', True), ('dz-none', False), ('dz-text', False), ('dz-both', True), ('dz-inner-text', False), ('dz-disabled', False), ('dz-inert', False), ('dz-plain', False)):
    lab.activate('ui4me-dnd — Dolphin'); time.sleep(0.3); t = time.time(); lab.dnd(*src_file(), *zone(z, top)); time.sleep(0.8); P('FILE ->', z, summ(t))
for z, top in (('dz-text', False), ('dz-files', True), ('dz-inner-text', False), ('dz-both', True), ('dz-none', False), ('dz-plain', False)):
    t = time.time(); lab.dnd(*src_text(), *zone(z, top)); time.sleep(0.8); P('TEXT ->', z, summ(t))
def gtk(mode, z, top=False, action=None, cap=C, win='main', settle=2.5):
    p = subprocess.Popen(['python3', 'gtk_dragsrc.py', mode], stderr=subprocess.DEVNULL)
    try:
        lab.wait_window(f'DragSrc {mode}'); lab.set_geometry(f'DragSrc {mode}', 100, 200, 300, 200); time.sleep(0.8)
        w = lab.window(f'DragSrc {mode}'); t = time.time(); lab.dnd(w['cx'] + 150, w['cy'] + 100, *zone(z, top, cap, win))
        a = action() if action else None; time.sleep(settle)
        return {'action': a, 'drops': [(e.get('element'), len(e.get('paths') or []), (e.get('paths') or [e.get('text')])[:1]) for e in c.doc_events(t, {'drop'})], 'errors': [e['data'].get('message') for e in c.events(t, 'document.error')]}
    finally:
        p.terminate(); p.wait(5)
P('mixed -> dz-text', gtk('mixed', 'dz-text')); P('mixed -> dz-files', gtk('mixed', 'dz-files', True))
P('https -> dz-files', gtk('https', 'dz-files', True)); P('1.1 MB text -> dz-text', gtk('bigtext', 'dz-text'))
P('1024 files', gtk('files1024', 'dz-files', True)); P('1025 files', gtk('files1025', 'dz-files', True)); P('malformed URIs', gtk('rawuri', 'dz-files', True))
P('delayed control', gtk('delayed', 'dz-text', settle=5))
P('delayed + receiver removed/recreated', gtk('delayed', 'dz-text', action=lambda: c.op('node_batch', ops=[{'op': 'remove_element', 'node': c.op('node_of', element='dz-text')['result']['node']}]).get('ok') and c.op('insert_html', element='s-c06', position='beforeend', html='<div id="dz-text" class="zone" data-ui4me-drop="text">recreated</div>').get('ok'), settle=5))
P('delayed + policy -> none', gtk('delayed', 'dz-text', action=lambda: c.op('set_attribute', element='dz-text', name='data-ui4me-drop', value='none').get('ok'), settle=5))
c.op('set_attribute', element='dz-text', name='data-ui4me-drop', value='text')
P('delayed + document replaced', gtk('delayed', 'dz-text', action=lambda: c.req({'kind': 'load', 'page': 'fixture.html'}).get('ok'), settle=5))
c.req({'kind': 'open', 'name': 'w2', 'page': 'fixture.html', 'title': 'UI4ME C-lab 2', 'width': 900, 'height': 700}); lab.wait_window('C-lab 2'); lab.set_geometry('C-lab 2', 1040, 40, 1000, 780); time.sleep(1)
P('delayed + target window closed', gtk('delayed', 'dz-text', action=lambda: js('C-lab 2', 'w.closeWindow()', 0.2) or 'closed', cap='C-lab 2', win='w2', settle=5), '| windows', [x['caption'] for x in lab.windows() if 'C-lab' in x['caption']])
for cap in ('source.txt', 'ui4me-dnd — Dolphin'): js(cap, 'w.closeWindow()')

P('## C11')
restart(2)
def wins(): return [(w['caption'][:40], w['res']) for w in lab.windows() if w['res'] in ('org.kde.kate', 'org.kde.dolphin') and 'ui4me-core' not in w['caption']]
P('open file', c.call('desktop.open_external', target=H + '/ui4me-test/složka s mezerou/soubor č.txt', window='main')); time.sleep(4); P('  ', wins()); lab.screenshot(f'{E}/C11-open-file.png')
P('open dir', c.call('desktop.open_external', target=H + '/ui4me-test/složka s mezerou', window='main')); time.sleep(4); P('  ', wins())
P('open https', c.call('desktop.open_external', target='https://example.org/ui4me-c11', window='main')); time.sleep(3)
for t in ('javascript:alert(1)', 'ftp://example.org/x', 'data:text/plain,hi'):
    r = c.call('desktop.open_external', target=t); P('deny', t, r.get('code'))
for p in ('ui4me-test/složka s mezerou/soubor č.txt', 'ui4me-test/složka s mezerou', 'ui4me-test/odkaz.txt'):
    P('reveal', p, c.call('desktop.reveal_file', path=H + '/' + p, window='main').get('result')); time.sleep(3)
P('  ', wins()); lab.screenshot(f'{E}/C11-reveal.png')
for cap in ('— Kate', 'složka s mezerou — Dolphin', 'ui4me-test — Dolphin'): js(cap, 'w.closeWindow()', 0.3)
time.sleep(2)
target = H + '/ui4me-test/blob-neznámý.ui4mexyz'; res = {}
def call(key, **kw):
    try: res[key] = c.call('desktop.open_external', target=target, **kw)
    except Exception as e: res[key] = repr(e)
chooser = lambda: [(w['caption'], w['transientFor']) for w in lab.windows() if 'Choose Application' in w['caption']]
th = threading.Thread(target=call, args=('esc',), kwargs={'window': 'main'}); th.start(); time.sleep(4); P('chooser', chooser()); lab.screenshot(f'{E}/C11-chooser.png'); lab.key('KEY_ESC'); th.join(20); P('Escape', res['esc'], chooser())
c.req({'kind': 'open', 'name': 'w2', 'page': 'a.html', 'title': 'UI4ME C-lab 2', 'width': 500, 'height': 300}); lab.wait_window('C-lab 2'); time.sleep(1)
th = threading.Thread(target=call, args=('parent',), kwargs={'window': 'w2'}); th.start(); time.sleep(4); P('chooser for w2', chooser())
js('C-lab 2', 'w.closeWindow()'); th.join(20); time.sleep(1); P('parent closed', res['parent'], chooser())
hp = lab.window(C)['pid']; th = threading.Thread(target=call, args=('quit',), kwargs={'window': 'main'}); th.start(); time.sleep(4); P('chooser pending', chooser())
P('request_quit', c.call('app.request_quit')); th.join(20); time.sleep(2)
P('pending result', res['quit'], '| chooser', chooser(), '| host alive', subprocess.run(['kill', '-0', str(hp)], capture_output=True).returncode == 0)
restart(3)
subprocess.run(['systemctl', '--user', 'mask', '--runtime', 'xdg-desktop-portal.service'], capture_output=True); subprocess.run(['systemctl', '--user', 'stop', 'xdg-desktop-portal.service']); time.sleep(2)
P('masked portal open_external', c.call('desktop.open_external', target=H + '/ui4me-test/external-open.txt'))
P('masked portal preferences', {k: v for k, v in c.call('desktop.preferences').get('result', {}).items() if k in ('available', 'source')})
subprocess.run(['systemctl', '--user', 'unmask', '--runtime', 'xdg-desktop-portal.service'], capture_output=True); subprocess.run(['systemctl', '--user', 'start', 'xdg-desktop-portal.service']); time.sleep(3)
r = c.call('desktop.preferences').get('result', {}); P('portal restored', r.get('available'), r.get('source'), r.get('color_scheme'), subprocess.run(['systemctl', '--user', 'is-active', 'xdg-desktop-portal.service'], capture_output=True, text=True).stdout.strip())
js(C, 'w.closeWindow()')
P('DONE')
