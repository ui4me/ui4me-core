#!/usr/bin/env python3
"""One UIA TextPattern operation through the native IUIAutomation COM client.

Complements UiaTextProbe.cs (managed System.Windows.Automation client). Every
UIA object is created and used on this process's main thread, initialised in
the apartment chosen by --apartment before comtypes is imported. Each step
writes a timestamped marker so an external watchdog can tell where a call
stalled. Requires the third-party `comtypes` package (diagnostics only).
"""
import argparse
from datetime import datetime, timezone
import sys

MODES = ("pattern", "document-text", "selection", "selection-text", "select", "select-read")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--automation-id")
    parser.add_argument("--class-name", help="select the first element of this native class (e.g. Edit)")
    parser.add_argument("--mode", choices=MODES, required=True)
    parser.add_argument("--apartment", choices=("MTA", "STA"), default="MTA")
    parser.add_argument("--log", required=True)
    args = parser.parse_args()
    # comtypes initialises COM on import using sys.coinit_flags.
    sys.coinit_flags = 0x0 if args.apartment == "MTA" else 0x2
    log = open(args.log, "w", encoding="utf-8", buffering=1)

    def mark(message):
        log.write(datetime.now(timezone.utc).isoformat() + " " + message + "\n")

    try:
        import comtypes
        import comtypes.client
        mark(f"START apartment={args.apartment} mode={args.mode}")
        uia_module = comtypes.client.GetModule("UIAutomationCore.dll")
        from comtypes.gen import UIAutomationClient as U
        automation = comtypes.client.CreateObject(U.CUIAutomation, interface=U.IUIAutomation)
        mark("BEFORE root windows")
        root = automation.GetRootElement()
        windows = root.FindAll(U.TreeScope_Children,
                               automation.CreatePropertyCondition(U.UIA_ProcessIdPropertyId, args.pid))
        mark(f"AFTER root windows count={windows.Length}")
        if windows.Length < 1:
            raise RuntimeError("target window not found")
        if args.automation_id:
            condition = automation.CreatePropertyCondition(U.UIA_AutomationIdPropertyId, args.automation_id)
        elif args.class_name:
            condition = automation.CreatePropertyCondition(U.UIA_ClassNamePropertyId, args.class_name)
        else:
            condition = automation.CreatePropertyCondition(U.UIA_ControlTypePropertyId, U.UIA_EditControlTypeId)
        element = None
        for index in range(windows.Length):
            element = windows.GetElement(index).FindFirst(U.TreeScope_Descendants, condition)
            if element:
                break
        mark("AFTER find control found=" + str(bool(element)))
        if not element:
            raise RuntimeError("text control not found")
        mark("BEFORE TextPattern")
        unknown = element.GetCurrentPattern(U.UIA_TextPatternId)
        if not unknown:
            raise RuntimeError("TextPattern unavailable")
        pattern = unknown.QueryInterface(U.IUIAutomationTextPattern)
        mark("AFTER TextPattern")
        if args.mode in ("selection", "selection-text"):
            mark("BEFORE GetSelection")
            ranges = pattern.GetSelection()
            count = ranges.Length if ranges else 0
            mark(f"AFTER GetSelection count={count}")
            if args.mode == "selection-text":
                for index in range(count):
                    mark(f"BEFORE selection[{index}].GetText")
                    text = ranges.GetElement(index).GetText(256)
                    mark(f"AFTER selection[{index}].GetText {text!r}")
        elif args.mode != "pattern":
            mark("BEFORE DocumentRange")
            document = pattern.DocumentRange
            mark("AFTER DocumentRange")
            mark("BEFORE DocumentRange.GetText")
            text = document.GetText(256)
            mark(f"AFTER DocumentRange.GetText {text!r}")
            if args.mode in ("select", "select-read"):
                mark("BEFORE Clone")
                selected = document.Clone()
                mark("BEFORE MoveEndpointByUnit")
                moved = selected.MoveEndpointByUnit(U.TextPatternRangeEndpoint_End, U.TextUnit_Character, -1)
                mark(f"AFTER MoveEndpointByUnit moved={moved}")
                mark("BEFORE Select")
                selected.Select()
                mark("AFTER Select")
                if args.mode == "select-read":
                    mark("BEFORE GetSelection")
                    ranges = pattern.GetSelection()
                    count = ranges.Length if ranges else 0
                    mark(f"AFTER GetSelection count={count}")
                    for index in range(count):
                        mark(f"BEFORE selection[{index}].GetText")
                        text = ranges.GetElement(index).GetText(256)
                        mark(f"AFTER selection[{index}].GetText {text!r}")
        del uia_module
        mark("PASS")
        return 0
    except Exception as error:  # the log is the diagnostic product
        mark(f"FAIL {error!r}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
