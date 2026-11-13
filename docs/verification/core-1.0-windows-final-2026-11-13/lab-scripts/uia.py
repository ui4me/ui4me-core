"""Native IUIAutomation (COM, MTA) helpers for the lab agent."""
import sys
import threading
import time

sys.coinit_flags = 0x0  # COINIT_MULTITHREADED for every thread that imports comtypes first
import comtypes  # noqa: E402
import comtypes.client  # noqa: E402

comtypes.client.GetModule("UIAutomationCore.dll")
from comtypes.gen import UIAutomationClient as U  # noqa: E402

_local = threading.local()
CT = {getattr(U, n): n[4:-13] for n in dir(U) if n.startswith("UIA_") and n.endswith("ControlTypeId")}


def A():
    if not getattr(_local, "a", None):
        comtypes.CoInitializeEx(0x0)
        _local.a = comtypes.client.CreateObject(U.CUIAutomation, interface=U.IUIAutomation)
    return _local.a


def window(pid=None, title=None):
    a = A()
    root = a.GetRootElement()
    cond = a.CreatePropertyCondition(U.UIA_ProcessIdPropertyId, pid) if pid else a.CreateTrueCondition()
    arr = root.FindAll(U.TreeScope_Children, cond)
    for i in range(arr.Length):
        e = arr.GetElement(i)
        if title is None or title in (e.CurrentName or ""):
            if e.CurrentName or title is not None:
                return e
    raise LookupError(f"no UIA window pid={pid} title={title!r}")


def find(root, automation_id=None, name=None, control=None):
    a = A()
    conds = []
    if automation_id is not None:
        conds.append(a.CreatePropertyCondition(U.UIA_AutomationIdPropertyId, automation_id))
    if name is not None:
        conds.append(a.CreatePropertyCondition(U.UIA_NamePropertyId, name))
    if control is not None:
        conds.append(a.CreatePropertyCondition(U.UIA_ControlTypePropertyId, getattr(U, f"UIA_{control}ControlTypeId")))
    cond = conds[0] if len(conds) == 1 else a.CreateAndConditionFromArray(conds)
    e = root.FindFirst(U.TreeScope_Descendants, cond)
    if not e:
        raise LookupError(f"element not found id={automation_id} name={name} control={control}")
    return e


def pattern(e, name):
    unk = e.GetCurrentPattern(getattr(U, f"UIA_{name}PatternId"))
    if not unk:
        return None
    return unk.QueryInterface(getattr(U, f"IUIAutomation{name}Pattern"))


def describe(e):
    r = e.CurrentBoundingRectangle
    d = {"name": e.CurrentName, "type": CT.get(e.CurrentControlType, e.CurrentControlType),
         "id": e.CurrentAutomationId, "class": e.CurrentClassName,
         "rect": [r.left, r.top, r.right - r.left, r.bottom - r.top],
         "focus": bool(e.CurrentHasKeyboardFocus), "enabled": bool(e.CurrentIsEnabled),
         "offscreen": bool(e.CurrentIsOffscreen), "help": e.CurrentHelpText or None}
    try:
        live = e.GetCurrentPropertyValue(U.UIA_LiveSettingPropertyId)
        if live:
            d["live"] = live
    except comtypes.COMError:
        pass
    for prop, key in ((U.UIA_IsRequiredForFormPropertyId, "required"), (U.UIA_ItemStatusPropertyId, "status"),
                      (U.UIA_LocalizedControlTypePropertyId, "localized"), (U.UIA_AriaRolePropertyId, "aria_role"),
                      (U.UIA_PositionInSetPropertyId, "pos"), (U.UIA_SizeOfSetPropertyId, "setsize"),
                      (U.UIA_LevelPropertyId, "level"), (U.UIA_FullDescriptionPropertyId, "description"),
                      (U.UIA_AriaPropertiesPropertyId, "aria")):
        try:
            v = e.GetCurrentPropertyValue(prop)
            if v not in (None, "", 0, False):
                d[key] = v
        except comtypes.COMError:
            pass
    v = pattern(e, "Value")
    if v:
        d["value"] = v.CurrentValue
        d["readonly"] = bool(v.CurrentIsReadOnly)
    t = pattern(e, "Toggle")
    if t:
        d["toggle"] = t.CurrentToggleState
    x = pattern(e, "ExpandCollapse")
    if x:
        d["expand"] = x.CurrentExpandCollapseState
    s = pattern(e, "SelectionItem")
    if s:
        d["selected"] = bool(s.CurrentIsSelected)
    rv = pattern(e, "RangeValue")
    if rv:
        d["range"] = [rv.CurrentMinimum, rv.CurrentValue, rv.CurrentMaximum]
    if pattern(e, "Invoke"):
        d["invoke"] = True
    if pattern(e, "Text"):
        d["text_pattern"] = True
    if pattern(e, "ScrollItem"):
        d["scroll_item"] = True
    lb = e.GetCurrentPropertyValue(U.UIA_LabeledByPropertyId)
    if lb:
        try:
            d["labeled_by"] = lb.QueryInterface(U.IUIAutomationElement).CurrentName
        except Exception:
            pass
    for prop, key in ((U.UIA_ControllerForPropertyId, "controller_for"), (U.UIA_DescribedByPropertyId, "described_by"),
                      (U.UIA_FlowsToPropertyId, "flows_to")):
        try:
            arr = e.GetCurrentPropertyValue(prop)
            if arr:
                d[key] = [x.QueryInterface(U.IUIAutomationElement).CurrentName for x in arr]
        except Exception:
            pass
    return d


def dump(root, depth=12, max_nodes=600):
    a = A()
    walker = a.ControlViewWalker
    out = []

    def rec(e, level):
        if len(out) >= max_nodes or level > depth:
            return
        d = describe(e)
        d["level"] = level
        out.append(d)
        c = walker.GetFirstChildElement(e)
        while c and len(out) < max_nodes:
            rec(c, level + 1)
            c = walker.GetNextSiblingElement(c)
    rec(root, 0)
    return out


def focused():
    return describe(A().GetFocusedElement())


def text_selection(e):
    tp = pattern(e, "Text")
    arr = tp.GetSelection()
    return [arr.GetElement(i).GetText(-1) for i in range(arr.Length if arr else 0)]


def document_text(e):
    return pattern(e, "Text").DocumentRange.GetText(-1)


def select_text(e, start, end):
    """Select characters [start, end) of the document range through TextPattern."""
    tp = pattern(e, "Text")
    r = tp.DocumentRange.Clone()
    total = len(r.GetText(-1))
    r.MoveEndpointByUnit(U.TextPatternRangeEndpoint_End, U.TextUnit_Character, end - total)
    r.MoveEndpointByUnit(U.TextPatternRangeEndpoint_Start, U.TextUnit_Character, start)
    r.Select()
    return r.GetText(-1)


class _Events:
    def __init__(self):
        self.items = []
        self.lock = threading.Lock()


_recorder = _Events()
_handlers = []


def listen(root, event_names=("LiveRegionChanged", "Text_TextSelectionChanged", "AutomationFocusChanged")):
    """Register automation event handlers on root's subtree; events go to events()."""
    a = A()

    class Handler(comtypes.COMObject):
        _com_interfaces_ = [U.IUIAutomationEventHandler]

        def __init__(self, name):
            super().__init__()
            self.name = name

        def HandleAutomationEvent(self, sender, event_id):
            try:
                info = {"name": sender.CurrentName, "id": sender.CurrentAutomationId}
            except Exception as error:
                info = {"error": repr(error)}
            with _recorder.lock:
                _recorder.items.append({"t": time.time(), "event": self.name, **info})
            return 0

    class Focus(comtypes.COMObject):
        _com_interfaces_ = [U.IUIAutomationFocusChangedEventHandler]

        def HandleFocusChangedEvent(self, sender):
            try:
                info = {"name": sender.CurrentName, "id": sender.CurrentAutomationId}
            except Exception as error:
                info = {"error": repr(error)}
            with _recorder.lock:
                _recorder.items.append({"t": time.time(), "event": "focus", **info})
            return 0

    for name in event_names:
        if name == "AutomationFocusChanged":
            h = Focus()
            a.AddFocusChangedEventHandler(None, h)
        else:
            h = Handler(name)
            a.AddAutomationEventHandler(getattr(U, f"UIA_{name}EventId"), root, U.TreeScope_Subtree, None, h)
        _handlers.append(h)
    return len(_handlers)


def events(since=0.0):
    with _recorder.lock:
        return [e for e in _recorder.items if e["t"] >= since]


def unlisten():
    A().RemoveAllEventHandlers()
    _handlers.clear()
