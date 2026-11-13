"""Final-binary group 2: R09, R07, N09/N10 (faults), N11, N14."""
import lab, ax, time, subprocess, json, os, re, signal
import hostinfo as hi
from gi.repository import Atspi
R = '/home/toor/projects/ui4me-core'
E = R + '/verification/core-1.0-linux-final/evidence'
F = R + '/verification/core-1.0-linux-final/faults'
HOST = os.path.expanduser('~/.cargo/bin/ui4me-host')
S = '/tmp/claude-1000/-home-toor-projects-ui4me-core/d6bab0d7-7bb7-4a6c-b84c-a5812eceb36c/scratchpad'
C = 'Desktop workspace'
def P(*a): print(*a, flush=True)
def close(cap):
    lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0) ws[i].closeWindow(); } OUT('x')" % json.dumps(cap))
def alive(pid): return subprocess.run(['kill', '-0', str(pid)], capture_output=True).returncode == 0
def wait_exit(pid, t=10):
    s = time.time()
    while time.time() - s < t and alive(pid): time.sleep(0.05)
    return round(time.time() - s, 2)

P('## R09 updates covered/minimized')
log = open(f'{E}/R09-updates-run.log', 'w')
proc = subprocess.Popen(['python3', 'tools/run_renderer.py', '--host', HOST, '--updates', '0.25'], cwd=R, stdout=log, stderr=subprocess.STDOUT)
w = lab.wait_window(C); hp = w['pid']; time.sleep(4); lab.set_geometry(C, 300, 100, 1000, 800); time.sleep(1)
status = lambda: [a.get_name() for _, a in ax.all_nodes(ax.find(ax.app('UI4ME Desktop Workspace'), 'status bar')) if a.get_role_name() == 'label']
upd = lambda: re.findall(r'UPDATE (\d+)', open(f'{E}/R09-updates-run.log').read())
lab.set_geometry('Konsole', 0, 0, 2048, 1036); lab.activate('Konsole'); time.sleep(1)
lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf('Konsole')>=0) ws[i].keepAbove=true; } OUT('x')")
c0, t0, u0, s0 = hi.ticks(hp), time.time(), upd(), status(); time.sleep(45); lab.screenshot(f'{E}/R09-main-fully-covered.png')
P(f'covered 45 s: host CPU {(hi.ticks(hp)-c0)/(time.time()-t0):.2f}% UPDATE {u0[-1:]} -> {upd()[-1:]} status {s0} -> {status()}')
lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf('Konsole')>=0) ws[i].keepAbove=false; } OUT('x')"); lab.set_geometry('Konsole', 0, 0, 795, 1036)
lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf('Desktop workspace')>=0) ws[i].minimized=true; } OUT('x')"); time.sleep(1)
c0, t0, u0, s0 = hi.ticks(hp), time.time(), upd(), status(); time.sleep(45)
P(f'minimized 45 s: host CPU {(hi.ticks(hp)-c0)/(time.time()-t0):.2f}% UPDATE {u0[-1:]} -> {upd()[-1:]} status {s0} -> {status()}')
lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf('Desktop workspace')>=0){ ws[i].minimized=false; workspace.activeWindow=ws[i]; } } OUT('x')"); time.sleep(1.5)
lab.screenshot(f'{E}/R09-restored.png'); P('restored status', status())
A = ax.app('UI4ME Desktop Workspace'); n = ax.find(A, 'entry', 'Name'); ax.click(n, C); lab.key('KEY_END'); lab.type_text(' ok'); time.sleep(0.4); P('typing after restore', repr(ax.text(n)))
lab.set_geometry('Konsole', 0, 0, 2048, 1036); lab.activate(C); time.sleep(0.5); lab.combo('KEY_LEFTCTRL', 'KEY_LEFTSHIFT', 'KEY_Q'); P('Ctrl+Shift+Q while covered: exit after', wait_exit(hp), 's'); lab.set_geometry('Konsole', 0, 0, 795, 1036)
proc.wait(20); log.close()

P('## R09 hold callback')
log = open(f'{E}/R09-hold-run.log', 'w')
proc = subprocess.Popen(['python3', 'tools/run_renderer.py', '--host', HOST, '--hold-callback'], cwd=R, stdout=log, stderr=subprocess.STDOUT)
w = lab.wait_window(C); hp = w['pid']; time.sleep(4); lab.set_geometry(C, 300, 100, 1000, 800); time.sleep(1)
A = ax.app('UI4ME Desktop Workspace'); ax.click(ax.find(A, 'button', 'Add row'), C); time.sleep(1.5)
P('HOLD marker', 'HOLD:' in open(f'{E}/R09-hold-run.log').read())
n = ax.find(A, 'entry', 'Notes'); ax.click(n, C); lab.type_text('typed while python is blocked'); time.sleep(0.5); P('typing while blocked', repr(ax.text(n)))
lab.wheel(-3); lab.screenshot(f'{E}/R09-hold-typing.png')
close(C); P('close while blocked: exit after', wait_exit(hp, 15), 's'); proc.wait(20); log.close()

P('## R09 crash after ready')
log = open(f'{E}/R09-crash-run.log', 'w')
proc = subprocess.Popen(['python3', 'tools/run_renderer.py', '--host', HOST, '--crash-after-ready', '4'], cwd=R, stdout=log, stderr=subprocess.STDOUT)
w = lab.wait_window(C); hp = w['pid']; lab.set_geometry(C, 300, 100, 1000, 700); time.sleep(7)
lab.screenshot(f'{E}/R09-crash-failure-ui.png'); P('window after crash', lab.window(C)['caption'], '| failure UI expected')
w = lab.window(C); lab.click(w['x'] + 440, w['y'] + 230); P('failure UI Quit: exit after', wait_exit(hp), 's'); proc.wait(20); log.close()

P('## R07 broken assets')
B = S + '/broken-app'
subprocess.run(['rm', '-rf', B]); subprocess.run(['cp', '-a', os.path.expanduser('~/.local/share/ui4me-core1-reference'), B])
os.remove(B + '/ui/landscape.png')
for f in os.listdir(B + '/ui/fonts'): open(B + '/ui/fonts/' + f, 'wb').write(os.urandom(2000))
t = open(B + '/app.toml').read().replace('org.ui4me.RendererReference', 'org.ui4me.RendererBroken'); open(B + '/app.toml', 'w').write(t)
log = open(f'{E}/R07-native.log', 'w'); proc = subprocess.Popen([HOST, 'run', '--app', B + '/app.toml'], cwd=os.path.expanduser('~'), stdout=log, stderr=subprocess.STDOUT)
w = lab.wait_window(C); hp = w['pid']; lab.set_geometry(C, 300, 60, 1100, 900); time.sleep(2); lab.screenshot(f'{E}/R07-broken-native.png')
A = ax.app('UI4ME Desktop Workspace'); n = ax.find(A, 'entry', 'Name'); ax.click(n, C); lab.key('KEY_END'); lab.type_text(' still usable'); time.sleep(0.3); P('typing', repr(ax.text(n)))
btn = ax.find(A, 'button', 'Open second window')
for i in range(10):
    Atspi.Action.do_action(btn, 0); time.sleep(0.6)
    if [x for x in lab.windows() if 'Second document' in x['caption']]: close('Second document'); time.sleep(0.3)
P('10 window cycles: windows', [x['caption'] for x in lab.windows() if x['pid'] == hp])
ax.click(ax.find(ax.app('UI4ME Desktop Workspace'), 'button', 'Quit'), C); P('Quit exit after', wait_exit(hp), 's'); proc.wait(20); log.close()
r = subprocess.run([HOST, 'render', '--html', B + '/ui/index.html', '--output', S + '/ev/R07.png', '--width', '1100', '--height', '900'], capture_output=True, text=True)
open(f'{E}/R07-broken-render.json', 'w').write(r.stdout); P('render diagnostics codes', sorted(set(re.findall(r'"(DECODE_FAILED|FILE_NOT_FOUND)"', r.stdout))))

P('## N09/N10 fault fixtures (native)')
os.makedirs(F, exist_ok=True)
for m in ('veto-close', 'veto-quit', 'freeze-close', 'freeze-quit', 'log-flood', 'managed-tree'):
    extra = ['--unread-host-output'] if m == 'log-flood' else []
    r = subprocess.run(['python3', 'tools/run_faults.py', m, '--host', HOST, '--duration', '2', '--timeout', '60', '--output', f'{F}/{m}'] + extra, cwd=R, capture_output=True, text=True, timeout=150)
    open(f'{F}/{m}.txt', 'w').write(r.stdout + r.stderr); P(m, 'rc', r.returncode)
r = subprocess.run(['python3', 'tools/run_faults.py', 'managed-tree', '--host', HOST, '--kill-host', '--timeout', '60', '--output', f'{F}/managed-tree-kill-host'], cwd=R, capture_output=True, text=True, timeout=150)
open(f'{F}/managed-tree-kill-host.txt', 'w').write(r.stdout + r.stderr); P('managed-tree --kill-host rc', r.returncode, 'descendants', json.loads(r.stdout[r.stdout.index('{'):])['managed_descendants'] if '{' in r.stdout else '?')
log = open(f'{F}/crash-after-ready.txt', 'w')
proc = subprocess.Popen(['python3', 'tools/run_faults.py', 'crash-after-ready', '--host', HOST, '--duration', '2', '--timeout', '60', '--output', f'{F}/crash-after-ready'], cwd=R, stdout=log, stderr=subprocess.STDOUT)
for i in range(40):
    ws = [w for w in lab.windows() if w['res'].startswith('org.ui4me')]
    if ws: break
    time.sleep(0.5)
time.sleep(5); lab.screenshot(f'{E}/N09-crash-failure-ui.png'); lab.activate(ws[0]['caption']); time.sleep(0.4); lab.combo('KEY_LEFTCTRL', 'KEY_Q')
P('crash-after-ready rc', proc.wait(90)); log.close()

P('## N11 external app survives host exit')
D = S + '/probe-ext'
def kate(): return [(p, open(f'/proc/{p}/cgroup').read().strip().split('/')[-1]) for p in subprocess.run(['pgrep', '-x', 'kate'], capture_output=True, text=True).stdout.split()]
for k, mode in enumerate(('quit', 'kill')):
    close('— Kate'); time.sleep(1); subprocess.run(['pkill', '-x', 'kate']); time.sleep(1)
    log = open(f'{E}/N11-run{k+1}.log', 'w'); proc = subprocess.Popen([HOST, 'run', '--app', D + '/app.toml'], cwd=os.path.expanduser('~'), stdout=log, stderr=subprocess.STDOUT)
    w = lab.wait_window('Python reference'); hp = w['pid']; time.sleep(1); lab.activate('Python reference'); time.sleep(0.4); lab.key('KEY_F8'); time.sleep(5)
    P(mode, 'after F8 kate', kate())
    if k == 0: lab.screenshot(f'{E}/N11-external-open.png'); lab.activate('Python reference'); time.sleep(0.4); lab.combo('KEY_LEFTCTRL', 'KEY_Q')
    else: os.kill(hp, signal.SIGKILL)
    time.sleep(3); P(mode, ': host alive', alive(hp), '| kate', kate(), '| probe backends', subprocess.run(['pgrep', '-fc', '^/usr/bin/python3.* .*core1-probe/demo.py'], capture_output=True, text=True).stdout.strip())
    proc.wait(20); log.close()
close('— Kate'); time.sleep(1)

P('## N14 nested realm')
nk = subprocess.Popen(['kwin_wayland', '--socket', 'ui4me-nested', '--width', '900', '--height', '650'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); time.sleep(4)
env = dict(os.environ, WAYLAND_DISPLAY='ui4me-nested')
ln = open(f'{E}/N14-nested.log', 'w'); pn = subprocess.Popen([HOST, 'run', '--app', os.path.expanduser('~/.local/share/ui4me-core1-probe/app.toml')], cwd=os.path.expanduser('~'), env=env, stdout=ln, stderr=subprocess.STDOUT); time.sleep(5)
lm = open(f'{E}/N14-main.log', 'w'); pm = subprocess.Popen([HOST, 'run', '--app', os.path.expanduser('~/.local/share/ui4me-core1-probe/app.toml')], cwd=os.path.expanduser('~'), stdout=lm, stderr=subprocess.STDOUT); time.sleep(5)
def backends():
    out = []
    for p in subprocess.run(['pgrep', '-f', '^/usr/bin/python3.* .*core1-probe/demo.py'], capture_output=True, text=True).stdout.split():
        pp = open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()[1]
        wd = [l for l in open(f'/proc/{pp}/environ', 'rb').read().split(b'\0') if l.startswith(b'WAYLAND_DISPLAY=')]
        out.append((p, pp, wd[0].decode() if wd else None, open(f'/proc/{pp}/cgroup').read().strip().split('/')[-1]))
    return out
P('both realms:', backends()); lab.screenshot(f'{E}/N14-two-realms.png')
P('main-realm windows', [w['caption'] for w in lab.windows() if w['res'] == 'org.ui4me.HostReference'])
lab.activate('Python reference'); time.sleep(0.4); lab.combo('KEY_LEFTCTRL', 'KEY_Q'); time.sleep(3)
P('after main-realm Ctrl+Q: main exit', pm.wait(20), '| remaining', backends())
nk.terminate(); nk.wait(10); time.sleep(4)
P('after nested compositor terminated: nested host exit', pn.wait(20), '| remaining', backends())
ln.close(); lm.close()
P('DONE')
