import lab, time, sys, json, os, subprocess, datetime, itertools
sys.path.insert(0, '../labapp'); import ctl, hostinfo as hi
from PIL import Image, ImageChops
E = '/home/toor/projects/ui4me-core/verification/core-1.0-linux-final/evidence'
LOG = '../labapp/host-final2.log'
C = 'RC2 lab'
def H(t): print('\n### ' + t, flush=True)
lab.wait_window(C); lab.set_geometry(C, 200, 40, 1200, 800); time.sleep(1)
hp = lab.window(C)['pid']; bp = hi.backend_pid()
print('host', hp, subprocess.check_output(['sha256sum', os.readlink(f'/proc/{hp}/exe')], text=True).split()[0])
ctl.run(open('../labapp/stress.py').read())

H('1000 displayed bundle PNG replacements (same size)')
ctl.run("result=state['bundle_loop'](40)"); time.sleep(1)
rows=[]
def sample(i):
    st=ctl.run("result=doc.resource_stats()"); rows.append({'replacements':i,'host_pss_kib':hi.pss(hp),'decoded_cache':st['cache']['bytes'],'cache_entries':st['cache']['entries']})
sample(0)
for k in range(20):
    ctl.run(f"result=state['bundle_loop'](50, start={k*50})"); sample((k+1)*50)
for r in rows[::4]+[rows[-1]]: print(json.dumps(r))
xs=[r['replacements'] for r in rows[2:]]; ys=[r['host_pss_kib'] for r in rows[2:]]; n=len(xs); mx=sum(xs)/n; my=sum(ys)/n
print(f'PSS slope after warmup: {sum((x-mx)*(y-my) for x,y in zip(xs,ys))/sum((x-mx)**2 for x in xs)*1024:.0f} bytes/replacement; total {rows[-1]["host_pss_kib"]-rows[0]["host_pss_kib"]} KiB over 1000; decoded cache constant {set(r["decoded_cache"] for r in rows)}')
json.dump(rows, open(f'{E}/RC2-bundle-soak.json','w'), indent=1)

H('intrinsic-size change and visible refresh')
X,Y=200,40
def shot(tag):
    time.sleep(0.6); return Image.open(lab.screenshot(f'{E}/RC2-{tag}.png')).convert('RGB')
def rect(im,color):
    px=im.load(); xs=[];ys=[]
    for y in range(Y,Y+700):
        for x in range(X,X+1100):
            if px[x,y]==color: xs.append(x); ys.append(y)
    return (min(xs),min(ys),max(xs)-min(xs)+1,max(ys)-min(ys)+1) if xs else None
for (w,h,col,tag) in ((200,100,(255,0,0),'red-200x100'),(200,100,(0,0,255),'blue-200x100'),(300,40,(0,160,0),'green-300x40'),(120,160,(255,200,0),'yellow-120x160')):
    ctl.run(f"doc.register_resources('lab', {{'pic.png': (state['png']({w},{h},{col+(255,)}),'image/png')}})"); im=shot(tag); print(tag, '->', rect(im,col))

H('same-URL bundled font replacement')
ctl.run(r'''
mono=open('/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf','rb').read(); serif=open('/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf','rb').read()
state['fonts']=(mono,serif)
doc.register_resources('lab', {'pic.png': (state['png'](120,160,(255,200,0,255)),'image/png'), 'font.ttf': (mono,'font/ttf')})
doc.add_stylesheet("@font-face{font-family:LabFont;src:url(ui4me-res://lab/font.ttf)} #ftext{font-family:LabFont}")''')
def ftext():
    r=ctl.run("result=doc.command('get_rect', element='ftext')")['rect']; w=lab.window(C)
    time.sleep(2.5); im=Image.open(lab.screenshot('/tmp/claude-1000/-home-toor-projects-ui4me-core/d6bab0d7-7bb7-4a6c-b84c-a5812eceb36c/scratchpad/ev/f.png')).convert('RGB')
    return im.crop((int(w['cx']+r['x']),int(w['cy']+r['y']),int(w['cx']+r['x']+r['width']),int(w['cy']+r['y']+r['height']))), r['width']
a,wa=ftext()
ctl.run("m,s=state['fonts']\ndoc.register_resources('lab', {'pic.png': (state['png'](120,160,(255,200,0,255)),'image/png'), 'font.ttf': (s,'font/ttf')})"); b,wb=ftext()
ctl.run("m,s=state['fonts']\ndoc.register_resources('lab', {'pic.png': (state['png'](120,160,(255,200,0,255)),'image/png'), 'font.ttf': (m,'font/ttf')})"); c2,wc=ftext()
print('text box width mono/serif-bold/mono:', wa, wb, wc, '| mono vs serif differ:', a.size!=b.size or ImageChops.difference(a,b.resize(a.size)).getbbox() is not None, '| mono restored identical:', a.size==c2.size and ImageChops.difference(a,c2).getbbox() is None)
im=Image.new('RGB',(max(a.width,b.width,c2.width), a.height+b.height+c2.height),'white'); im.paste(a,(0,0)); im.paste(b,(0,a.height)); im.paste(c2,(0,a.height+b.height)); im.save(f'{E}/RC2-font-compare.png')

H('close veto (HTML button, compositor close, Alt+F4)')
ctl.run("state['veto']=True"); lab.set_geometry(C, 200, 40, 1200, 800); time.sleep(0.6); w=lab.window(C)
t=time.time(); lab.click(w['x']+w['w']-26, w['y']+20); time.sleep(1); print('× button with veto: still open', lab.window(C) is not None, len([e for e in ctl.events(t) if e['kind']=='before_close']))
t=time.time(); lab.kwin_script("var ws=workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf('RC2 lab')>=0) ws[i].closeWindow(); } OUT('x')"); time.sleep(1); print('compositor close with veto: still open', lab.window(C) is not None, len([e for e in ctl.events(t) if e['kind']=='before_close']))
lab.activate(C); time.sleep(0.3); t=time.time(); lab.combo('KEY_LEFTALT','KEY_F4'); time.sleep(1); print('Alt+F4 with veto: still open', lab.window(C) is not None, len([e for e in ctl.events(t) if e['kind']=='before_close']))
ctl.run("state['veto']=False")

H('scale 1.5 / 2 / 1 with streaming bitmap')
ctl.run(r'''
import threading as _th
for b in list(doc._bitmaps): b.close()
bm=doc.create_bitmap('b1', 64, 64); fr=state['frames_for'](64,64); stop=_th.Event()
def prod():
    i=0
    while not stop.wait(1/30): bm.present(fr[i%4]); i+=1
th=_th.Thread(target=prod,daemon=True); th.start(); state['sc']=(stop,th,bm)''')
for scale,(gw,gh) in ((1.5,(880,600)),(2,(600,400)),(1,(900,640))):
    subprocess.run(['kscreen-doctor', f'output.HDMI-A-1.scale.{scale}'], capture_output=True); time.sleep(3); lab.set_scale(scale)
    lab.set_geometry(C, 40, 20, gw, gh); time.sleep(1.5); w=lab.window(C); st=ctl.run("result=window.state()")
    t=time.time(); lab.click(w['x']+112, w['y']+112); time.sleep(0.6); clk=[(e['e']['element'],round(e['e']['x']),round(e['e']['y'])) for e in ctl.events(t) if e['kind']=='event' and e['type']=='click']
    lab.drag(w['x']+150,w['y']+20,w['x']+250,w['y']+80,steps=20); time.sleep(1); w2=lab.window(C)
    X,Y,W,Hh=w2['x'],w2['y'],w2['w'],w2['h']; lab.drag(X+W-4,Y+Hh-4,X+W-104,Y+Hh-54,steps=20); time.sleep(1); w3=lab.window(C)
    print(f"scale {scale}: scale_factor {st['scale_factor']} physical {st['physical_size']} | pad click {clk} | title drag {(round(w2['x']-w['x']),round(w2['y']-w['y']))} | SE resize {(round(w3['w']-W),round(w3['h']-Hh))}")
print('frames during scale checks', ctl.run("stop,th,bm=state['sc']; stop.set(); th.join(3); result=bm._sequence\nbm.close()"))
print('scale restored', subprocess.run(['kscreen-doctor','-o'],capture_output=True,text=True).stdout.split('Scale:')[1].split('\n')[0].strip()[-1])

H('resize-to-present latency and cost: static / CSS animation / 60 fps bitmap; idle return')
def frames_since(t0):
    out=[]
    for l in open(LOG):
        d=json.loads(l)
        if d['fields'].get('message')=='frame presented' and d['level']=='DEBUG':
            if datetime.datetime.fromisoformat(d['timestamp'].replace('Z','+00:00')).timestamp()>=t0: out.append(d['fields'])
    return out
q=lambda a,p: round(sorted(a)[min(len(a)-1,int(p*len(a)))],2) if a else None
def idle(tag, secs=15):
    time.sleep(2); c0,t0=hi.ticks(hp),time.time(); time.sleep(secs); print(json.dumps({'tag':tag,'host_cpu_pct':round((hi.ticks(hp)-c0)/(time.time()-t0),2),'frames':len(frames_since(t0))}))
def resize(tag):
    lab.set_geometry(C, 200, 60, 1000, 700); time.sleep(1.0); t0=time.time(); c0=hi.ticks(hp)
    for k in range(3):
        w=lab.window(C); lab.drag(w['x']+w['w']-5,w['y']+w['h']-5,w['x']+w['w']-305,w['y']+w['h']-205,steps=40,dt=0.016); time.sleep(0.3)
        w=lab.window(C); lab.drag(w['x']+w['w']-5,w['y']+w['h']-5,w['x']+w['w']+295,w['y']+w['h']+195,steps=40,dt=0.016); time.sleep(0.3)
    fr=frames_since(t0); lat=[f['resize_to_present_ms'] for f in fr if f.get('resize_to_present_ms') is not None]; fms=[f['frame_ms'] for f in fr]
    print(json.dumps({'tag':tag,'frames':len(fr),'distinct_sizes':len([k for k,_ in itertools.groupby((f['width'],f['height']) for f in fr)]),'resize_to_present_ms':{'p50':q(lat,.5),'p95':q(lat,.95),'max':q(lat,1)},'frame_ms':{'p50':q(fms,.5),'p95':q(fms,.95)},'host_cpu_pct':round((hi.ticks(hp)-c0)/(time.time()-t0),1)}))
lab.set_geometry(C, 200, 60, 1000, 700); idle('static idle')
resize('resize: static')
s=ctl.run('result=doc.add_stylesheet("@keyframes spin{to{transform:rotate(360deg)}} #pad{animation:spin 2s linear infinite}")')
idle('CSS animation running',10); resize('resize: CSS animation'); ctl.run(f"doc.remove_stylesheet({s['sheet']})"); idle('after animation removed')
ctl.run(r'''
import threading as _th
bm=doc.create_bitmap('b1', 320, 200); fr=state['frames_for'](320,200); stop=_th.Event()
def prod():
    i=0; import time as _t; t=_t.perf_counter()
    while not stop.is_set():
        bm.present(fr[i%4]); i+=1; d=t+i/60-_t.perf_counter()
        if d>0: stop.wait(d)
th=_th.Thread(target=prod,daemon=True); th.start(); state['sc']=(stop,th,bm)''')
idle('60 fps 320x200 bitmap',10); resize('resize: 60 fps bitmap')
for v in ('false','true'):
    subprocess.run(['busctl','--user','set-property','org.a11y.Bus','/org/a11y/bus','org.a11y.Status','IsEnabled','b',v]); time.sleep(2); idle(f'60 fps bitmap, AT-SPI IsEnabled={v}',10)
ctl.run("stop,th,bm=state['sc']; stop.set(); th.join(3); bm.close()"); idle('after bitmap closed')
