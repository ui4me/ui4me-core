// Separate-process UIA diagnostic. Build with uia_text_probe.ps1. Every UIA
// object is created and used on the same dedicated, explicitly chosen thread.
using System;
using System.IO;
using System.Text;
using System.Threading;
using System.Windows.Automation;
using System.Windows.Automation.Text;

public static class UiaTextProbe
{
    private static void Mark(StreamWriter log, string message)
    {
        log.WriteLine(DateTime.UtcNow.ToString("o") + " " + message);
        log.Flush();
    }

    public static int Run(int processId, string windowTitle, string automationId,
        string controlName, string mode, string apartment, bool focus,
        string logPath, int timeoutSeconds)
    {
        int result = 1;
        var worker = new Thread(delegate() {
            using (var log = new StreamWriter(logPath, false, new UTF8Encoding(false))) {
                try {
                    Mark(log, "START pid=" + System.Diagnostics.Process.GetCurrentProcess().Id +
                         " apartment=" + Thread.CurrentThread.GetApartmentState() + " mode=" + mode);
                    Mark(log, "BEFORE root windows");
                    var roots = AutomationElement.RootElement.FindAll(TreeScope.Children,
                        processId > 0 ? (Condition)new PropertyCondition(AutomationElement.ProcessIdProperty, processId)
                                      : Condition.TrueCondition);
                    Mark(log, "AFTER root windows count=" + roots.Count);
                    AutomationElement window = null;
                    foreach (AutomationElement candidate in roots) {
                        Mark(log, "BEFORE window name");
                        string name = candidate.Current.Name;
                        Mark(log, "AFTER window name=" + name);
                        if (String.IsNullOrEmpty(windowTitle) || name.IndexOf(windowTitle, StringComparison.OrdinalIgnoreCase) >= 0) {
                            if (window != null) throw new InvalidOperationException("More than one matching window; give an exact process and title.");
                            window = candidate;
                        }
                    }
                    if (window == null) throw new InvalidOperationException("Target window not found in this desktop/session.");
                    Condition controlCondition = !String.IsNullOrEmpty(automationId)
                        ? (Condition)new PropertyCondition(AutomationElement.AutomationIdProperty, automationId)
                        : !String.IsNullOrEmpty(controlName)
                            ? (Condition)new PropertyCondition(AutomationElement.NameProperty, controlName)
                            : new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Edit);
                    Mark(log, "BEFORE find control");
                    var element = window.FindFirst(TreeScope.Descendants, controlCondition);
                    Mark(log, "AFTER find control");
                    if (element == null) throw new InvalidOperationException("Text control not found.");
                    if (focus) {
                        Mark(log, "BEFORE SetFocus");
                        element.SetFocus();
                        Mark(log, "AFTER SetFocus");
                    }
                    Mark(log, "BEFORE TextPattern");
                    var pattern = (TextPattern)element.GetCurrentPattern(TextPattern.Pattern);
                    Mark(log, "AFTER TextPattern");
                    if (mode == "pattern") { result = 0; Mark(log, "PASS"); return; }

                    if (mode == "selection" || mode == "selection-text") {
                        // Crucially, this branch never calls Select/DocumentRange/GetText first.
                        Mark(log, "BEFORE GetSelection");
                        var ranges = pattern.GetSelection();
                        Mark(log, "AFTER GetSelection count=" + ranges.Length);
                        if (mode == "selection-text") {
                            for (int i = 0; i < ranges.Length; i++) {
                                Mark(log, "BEFORE selection[" + i + "].GetText");
                                string selected = ranges[i].GetText(256);
                                Mark(log, "AFTER selection[" + i + "].GetText UTF16=" + selected.Length);
                            }
                        }
                    } else {
                        Mark(log, "BEFORE DocumentRange");
                        var range = pattern.DocumentRange;
                        Mark(log, "AFTER DocumentRange");
                        if (mode == "document-text") {
                            Mark(log, "BEFORE DocumentRange.GetText");
                            string text = range.GetText(256);
                            Mark(log, "AFTER DocumentRange.GetText UTF16=" + text.Length);
                        } else if (mode == "select") {
                            Mark(log, "BEFORE Clone");
                            range = range.Clone();
                            Mark(log, "AFTER Clone");
                            Mark(log, "BEFORE MoveEndpointByUnit");
                            int moved = range.MoveEndpointByUnit(TextPatternRangeEndpoint.End, TextUnit.Character, -1);
                            Mark(log, "AFTER MoveEndpointByUnit moved=" + moved);
                            Mark(log, "BEFORE Select");
                            range.Select();
                            Mark(log, "AFTER Select");
                            // Selection reading is deliberately another process/mode.
                        } else throw new ArgumentException("Unknown mode: " + mode);
                    }
                    Mark(log, "PASS");
                    result = 0;
                } catch (Exception error) {
                    Mark(log, "FAIL " + error.ToString());
                    result = 1;
                }
            }
        });
        worker.IsBackground = true;
        worker.SetApartmentState(apartment == "MTA" ? ApartmentState.MTA : ApartmentState.STA);
        worker.Start();
        // The external process watchdog is authoritative if COM also prevents
        // shutdown. This deadline merely lets an ordinary stuck worker return.
        if (!worker.Join(TimeSpan.FromSeconds(timeoutSeconds))) return 124;
        return result;
    }
}
