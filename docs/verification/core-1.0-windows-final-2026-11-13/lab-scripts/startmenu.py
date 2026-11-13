import time, wl
from wl import P, key, uia, ex
def start_launch(name, shot=None):
    key("super"); time.sleep(2.0)
    pos = uia(f'''
import time
a = uia.A()
deadline = time.time() + 8
found = None
while time.time() < deadline and not found:
    st = a.GetRootElement().FindFirst(uia.U.TreeScope_Children, a.CreatePropertyCondition(uia.U.UIA_NamePropertyId, "Start"))
    if st:
        for d in uia.dump(st, depth=12, max_nodes=200):
            if d["type"] == "ListItem" and d["name"] and d["name"].split(",")[0] == {name!r} and d["rect"][2] > 0:
                found = [d["rect"][0] + d["rect"][2] / 2, d["rect"][1] + d["rect"][3] / 2, d["name"]]
                break
    time.sleep(0.3)
result = found''')
    if shot:
        wl.wshot(shot)
    P("start menu entry", pos)
    if not pos:
        key("Escape"); raise SystemExit("start menu entry not found")
    wl.click(pos[0], pos[1]); time.sleep(4)
if __name__ == "__main__":
    start_launch("UI4ME Desktop Workspace", "final\\N01-start-entry.png")
    P(wl.win("Desktop workspace", 30)["title"])
