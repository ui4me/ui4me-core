"""RC2 final-binary suite: frameless chrome, buttons, live bitmap, bundles, fonts, scale, veto, perf."""
import lab, time, sys, json, os, subprocess, signal, datetime, itertools
sys.path.insert(0, '../labapp'); import ctl, hostinfo as hi
from PIL import Image
E = '/home/toor/projects/ui4me-core/verification/core-1.0-linux-final/evidence'
LOG = '../labapp/host-final.log'
C = 'RC2 lab'
files = lambda: sorted(f for f in os.listdir('/run/user/1000') if f.startswith('ui4me-bitmap-'))
def H(t): print('\n### ' + t, flush=True)
lab.wait_window(C); lab.set_geometry(C, 500, 200, 900, 640); time.sleep(1)
hp = lab.window(C)['pid']; bp = hi.backend_pid()
print('host', hp, os.readlink(f'/proc/{hp}/exe'), subprocess.check_output(['sha256sum', os.readlink(f'/proc/{hp}/exe')], text=True).split()[0], 'backend', bp)
ctl.run(open('../labapp/stress.py').read())

H('8 native resize directions')
D = 60
cases = {'n': (lambda X,Y,W,H:(X+W/2,Y+2),(0,-D)), 's': (lambda X,Y,W,H:(X+W/2,Y+H-3),(0,D)), 'e': (lambda X,Y,W,H:(X+W-3,Y+H/2),(D,0)),
         'w': (lambda X,Y,W,H:(X+2,Y+H/2),(-D,0)), 'ne': (lambda X,Y,W,H:(X+W-4,Y+3),(D,-D)), 'nw': (lambda X,Y,W,H:(X+3,Y+3),(-D,-D)),
         'se': (lambda X,Y,W,H:(X+W-4,Y+H-4),(D,D)), 'sw': (lambda X,Y,W,H:(X+3,Y+H-4),(-D,D))}
def g(): w=lab.window(C); return (round(w['x']),round(w['y']),round(w['w']),round(w['h']))
allok = True
for name,(pos,(dx,dy)) in cases.items():
    lab.set_geometry(C, 500, 200, 900, 640); time.sleep(0.6); X,Y,W,Hh = g(); px,py = pos(X,Y,W,Hh)
    lab.drag(px,py,px+dx,py+dy,steps=25); time.sleep(0.6); X2,Y2,W2,H2 = g()
    ew = W+(dx if 'e' in name else -dx if 'w' in name else 0); eh = Hh+(dy if 's' in name else -dy if 'n' in name else 0)
    ex = X+(dx if 'w' in name else 0); ey = Y+(dy if 'n' in name else 0)
    ok = max(abs(W2-ew),abs(H2-eh),abs(X2-ex),abs(Y2-ey)) <= 2; allok &= ok
    print(f'{name:3} {(X,Y,W,Hh)} -> {(X2,Y2,W2,H2)} expected {(ex,ey,ew,eh)} {"PASS" if ok else "FAIL"}')
print('ALL 8 DIRECTIONS', 'PASS' if allok else 'FAIL')

H('title drag, double-click, buttons, right/middle, title input')
lab.set_geometry(C, 500, 200, 900, 640); time.sleep(0.6); w = lab.window(C)
lab.drag(w['x']+150,w['y']+20,w['x']-150,w['y']+170,steps=30); time.sleep(0.6); w2=lab.window(C); print('title drag moved', (round(w2['x']-w['x']),round(w2['y']-w['y'])))
lab.dblclick(w2['x']+150,w2['y']+20); time.sleep(1); print('dblclick -> maximized', lab.window(C)['maximized'])
w3=lab.window(C); lab.dblclick(w3['x']+150,w3['y']+20); time.sleep(1); print('dblclick -> maximized', lab.window(C)['maximized'])
lab.set_geometry(C, 500, 200, 900, 640); time.sleep(0.6); w=lab.window(C); X,Y=w['x'],w['y']
pad=(X+112,Y+112)
for b in ('left','right','middle'):
    t=time.time(); lab.click(*pad,name=b); time.sleep(0.5)
    print(b, [(e['type'],e['e'].get('button')) for e in ctl.events(t) if e['kind']=='event'])
for b in ('right','middle'):
    w0=lab.window(C); lab.drag(X+150,Y+20,X+350,Y+120,name=b,steps=20); time.sleep(0.5); w1=lab.window(C); print(f'{b}-drag on title moved window:', (w0['x'],w0['y'])!=(w1['x'],w1['y'])); lab.key('KEY_ESC')
w0=lab.window(C); W=w0['w']; ix=X+W-4-3*40-3*6-110+10
lab.drag(ix,Y+20,ix+80,Y+80,steps=20); time.sleep(0.5); w1=lab.window(C); print('drag inside title input moved window:', (w0['x'],w0['y'])!=(w1['x'],w1['y']), '| selection', ctl.run("result=doc.control_selection('tin')"))

H('live bitmap: 1000+ frames x 3 sizes, newest frame, memory')
lab.set_geometry(C, 200, 40, 1400, 1000); time.sleep(0.8)
def snap(tag):
    d={'tag':tag,'host_pss_kib':hi.pss(hp),'host_threads':hi.threads(hp),'host_fds':hi.fds(hp),'backend_pss_kib':hi.pss(bp),'bitmap_files':files()}; print(json.dumps(d)); return d
def fpx(k,x,w): return (((x*255//max(w-1,1))+k*60)&255,(k*80)&255,255-(x*255//max(w-1,1)))
def newest(w,h,count):
    time.sleep(0.4); p=lab.screenshot(f'{E}/RC2-bitmap-{w}x{h}.png'); im=Image.open(p).convert('RGB'); px=im.load(); W,Hh=im.size; k=(count-1)%4
    for y in range(40,Hh):
        for x in range(200,W-w):
            if px[x,y]==fpx(k,0,w) and px[x+1,y]==fpx(k,1,w) and px[x+w-1,y]==fpx(k,w-1,w):
                return {kk:sum(px[x+xx,y+yy]==fpx(kk,xx,w) for xx in range(0,w,max(1,w//32)) for yy in range(0,h,max(1,h//32))) for kk in range(4)}
    return None
snap('baseline')
for (w,h,n) in ((64,64,1501),(320,200,1502),(1024,768,1003),(1024,768,1000),(1024,768,1001)):
    if w==1024: lab.set_geometry(C, 0, 0, 1700, 1000); time.sleep(0.5)
    c0,t0=hi.ticks(hp),time.time(); r=ctl.run(f"result=state['stress']('b1',{w},{h},{n})"); r['host_cpu_pct']=round((hi.ticks(hp)-c0)/(time.time()-t0),1)
    print(json.dumps(r))
    if w!=1024: print('  newest-frame sample matches per frame index:', newest(w,h,n))
    snap(f'after {w}x{h} x{n}')

H('bitmap error semantics')
print(json.dumps(ctl.run(r'''
rpc = app.rpc; out = {}
for b in list(doc._bitmaps): b.close()
bm = doc.create_bitmap('b1', 64, 64); fr = state['frames_for'](64, 64)
base = {"window_id": window.id, "document_id": doc.id, "bitmap_id": "b1"}
def call(m, **kw):
    try: return rpc.call(m, {**base, **kw})
    except Exception as e: return {"error": [getattr(e, "code", None), str(e)]}
bm._map[0:64*64*4] = fr[0]
out["present_seq5"] = call("bitmap.present", sequence=5, slot=0)
out["stale_seq3"] = call("bitmap.present", sequence=3, slot=1)
out["bad_slot"] = call("bitmap.present", sequence=6, slot=2)
out["zero_seq"] = call("bitmap.present", sequence=0, slot=0)
out["missing_bitmap"] = call("bitmap.present", sequence=7, slot=0, bitmap_id="nope")
out["stale_document"] = call("bitmap.present", sequence=7, slot=0, document_id="stale")
out["missing_window"] = call("bitmap.present", sequence=7, slot=0, window_id="w999")
out["duplicate_create"] = call("bitmap.create", width=8, height=8)
out["too_big"] = call("bitmap.create", bitmap_id="huge", width=4096, height=4096, slots=2)
out["bad_format"] = call("bitmap.create", bitmap_id="fmt", width=8, height=8, pixel_format="BGRA8")
out["ninth"] = [("ok" if "error" not in call("bitmap.create", bitmap_id=f"m{i}", width=8, height=8) else "limit") for i in range(8)]
for i in range(8): call("bitmap.close", bitmap_id=f"m{i}")
big=[]
for i in range(3):
    try: doc.create_bitmap(f"big{i}", 2048, 2048, slots=2); big.append("ok")
    except Exception as e: big.append(str(e)[:60])
out["aggregate_64MiB"] = big
for b in list(doc._bitmaps): b.close()
out["present_after_close"] = call("bitmap.present", sequence=99, slot=0)
result = out'''), indent=0))

H('overload (unthrottled 2-thread producer at 1024x768) with live resize; responsiveness')
lab.set_geometry(C, 100, 40, 1300, 900); time.sleep(0.6); w=lab.window(C); px0,py0=w['x']+100,w['y']+100
def clicks(n=6):
    out=[]
    for i in range(n):
        lab.move(px0,py0); time.sleep(0.1); t=time.time(); lab.button('left',True); lab.button('left',False)
        end=time.time()+5
        while time.time()<end:
            ev=[e for e in ctl.events(t) if e['kind']=='event' and e['type']=='click']
            if ev: out.append(round((ev[0]['t']-t)*1000,1)); break
            time.sleep(0.002)
        time.sleep(0.2)
    return sorted(out)
print('idle click->python ms', clicks())
ctl.run(r'''
import threading as _th
bm = doc.create_bitmap('b1', 1024, 768, slots=2); fr = state['frames_for'](1024, 768)
st = {"n": 0, "drops": 0}; stop = _th.Event()
def prod(k):
    i=k
    while not stop.is_set():
        if k == 0: bm.present(fr[i % 4]); st["n"] += 1
        else:
            r = bm.try_present(fr[i % 4]); st["drops" if r is None else "n"] += 1
            stop.wait(0.001)
        i += 1
ths=[_th.Thread(target=prod,args=(k,),daemon=True) for k in range(2)]
for t in ths: t.start()
state["ov"]=(stop,ths,st,bm)''')
time.sleep(1); print('overloaded click->python ms', clicks())
samples=[]
for k in range(4):
    w=lab.window(C); X,Y,W,Hh=w['x'],w['y'],w['w'],w['h']
    lab.drag(X+W-4,Y+Hh-4,X+W-400,Y+Hh-300,steps=30,dt=0.02); w=lab.window(C); lab.drag(w['x']+w['w']-4,w['y']+w['h']-4,w['x']+w['w']+396,w['y']+w['h']+296,steps=30,dt=0.02)
    samples.append((hi.pss(hp),hi.threads(hp),hi.fds(hp)))
r=ctl.run('stop,ths,st,bm=state["ov"]; stop.set()\nfor t in ths: t.join(5)\nresult={"presented":st["n"],"try_present_dropped":st["drops"],"seq":bm._sequence}\nbm.close()')
print('overload result', r, '| host (PSS KiB, threads, fds) during resize cycles', samples)

H('teardown: document replacement, backend SIGKILL')
r=ctl.run(r'''
a=doc.create_bitmap('b1', 64, 64); b=doc.create_bitmap('b2', 32, 32); a.present(bytes(64*64*4))
html=(HERE/'ui'/'lab.html').read_text(); bms=list(doc._bitmaps); new=window.load_html(html); state['doc2']=new
c=new.create_bitmap('b1', 64, 64); c.present(bytes(64*64*4)); state['c']=c
result={"old_closed":[x._closed for x in bms]}''')
print('document replaced:', r, '| files', files(), '| host fds to bitmaps', [os.readlink(f'/proc/{hp}/fd/{fd}') for fd in os.listdir(f'/proc/{hp}/fd') if 'bitmap' in os.readlink(f'/proc/{hp}/fd/{fd}')])
os.kill(bp, signal.SIGKILL); time.sleep(2)
print('backend SIGKILL: host alive', os.path.exists(f'/proc/{hp}'), '| host bitmap fds', [fd for fd in os.listdir(f'/proc/{hp}/fd') if 'bitmap' in os.readlink(f'/proc/{hp}/fd/{fd}')], '| files', files())
lab.screenshot(f'{E}/RC2-backend-killed.png')
