import gi, sys, json
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi

def app(name_part):
    d = Atspi.get_desktop(0)
    for i in range(d.get_child_count()):
        a = d.get_child_at_index(i)
        if a and (a.get_name() or '') == name_part:
            return a
    return None

def walk(acc, depth=0, out=None, maxdepth=40):
    out = [] if out is None else out
    try:
        role = acc.get_role_name(); name = acc.get_name()
    except Exception as e:
        out.append({'depth': depth, 'error': str(e)}); return out
    item = {'depth': depth, 'role': role, 'name': name}
    try:
        if acc.get_text_iface() is not None:
            n = Atspi.Text.get_character_count(acc)
            item['text'] = Atspi.Text.get_text(acc, 0, min(n, 200))
    except Exception: pass
    try:
        v = acc.get_value_iface()
        if v: item['value'] = v.get_current_value()
    except Exception: pass
    try:
        st = acc.get_state_set()
        item['states'] = [s.value_nick for s in Atspi.StateType.__enum_values__.values() if st.contains(s)][:12]
    except Exception: pass
    try:
        attrs = acc.get_attributes()
        if attrs: item['attrs'] = dict(attrs)
    except Exception: pass
    out.append(item)
    if depth < maxdepth:
        for i in range(acc.get_child_count()):
            c = acc.get_child_at_index(i)
            if c: walk(c, depth + 1, out, maxdepth)
    return out

if __name__ == '__main__':
    a = app(sys.argv[1])
    print(json.dumps({'app': a.get_name() if a else None, 'tree': walk(a) if a else []}, ensure_ascii=False, indent=0))
