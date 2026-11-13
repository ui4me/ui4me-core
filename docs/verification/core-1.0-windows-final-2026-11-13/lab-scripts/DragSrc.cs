// Test-only OLE drag source (WinForms). A real mouse press in the window starts
// DoDragDrop with the data selected by mode. "delayed" renders text 3 s late.
using System;
using System.Collections.Specialized;
using System.IO;
using System.Threading;
using System.Windows.Forms;

public class DelayedData : IDataObject {
    public static string Log;
    public object GetData(string format, bool autoConvert) { File.AppendAllText(Log, DateTime.UtcNow.ToString("o") + " GetData " + format + Environment.NewLine); if (format == DataFormats.UnicodeText || format == DataFormats.Text) { Thread.Sleep(3000); File.AppendAllText(Log, DateTime.UtcNow.ToString("o") + " GetData returns" + Environment.NewLine); return "delayed text payload"; } return null; }
    public object GetData(string format) { return GetData(format, true); }
    public object GetData(Type format) { return null; }
    public void SetData(string format, bool autoConvert, object data) { }
    public void SetData(string format, object data) { }
    public void SetData(Type format, object data) { }
    public void SetData(object data) { }
    public bool GetDataPresent(string format, bool autoConvert) { return format == DataFormats.UnicodeText || format == DataFormats.Text; }
    public bool GetDataPresent(string format) { return GetDataPresent(format, true); }
    public bool GetDataPresent(Type format) { return false; }
    public string[] GetFormats(bool autoConvert) { return new string[] { DataFormats.UnicodeText }; }
    public string[] GetFormats() { return GetFormats(true); }
}

public static class DragSrc {
    static StringCollection Files(string dir, int n) {
        Directory.CreateDirectory(dir);
        var c = new StringCollection();
        for (int i = 0; i < n; i++) { var p = Path.Combine(dir, "f" + i.ToString("D4") + ".txt"); if (!File.Exists(p)) File.WriteAllText(p, "x"); c.Add(p); }
        return c;
    }
    public static void Run(string mode, string root, string log) {
        DelayedData.Log = log;
        var form = new Form { Text = "DragSrc " + mode, Width = 300, Height = 200, TopMost = true };
        form.MouseDown += (s, e) => {
            object data;
            var d = new DataObject();
            switch (mode) {
                case "mixed": { var c = new StringCollection(); c.Add(Path.Combine(root, "dnd", "žluťoučký 日本.txt")); d.SetFileDropList(c); d.SetText("mixed text part", TextDataFormat.UnicodeText); data = d; break; }
                case "https": d.SetText("https://example.org/ui4me-dnd", TextDataFormat.UnicodeText); data = d; break;
                case "bigtext": d.SetText(new string('a', 1100 * 1024), TextDataFormat.UnicodeText); data = d; break;
                case "files1024": d.SetFileDropList(Files(Path.Combine(root, "many"), 1024)); data = d; break;
                case "files1025": d.SetFileDropList(Files(Path.Combine(root, "many"), 1025)); data = d; break;
                case "oddpaths": { var c = new StringCollection(); c.Add(Path.Combine(root, "dnd", "missing-ž.txt")); c.Add(@"\\?\" + Path.Combine(root, "dnd", "žluťoučký 日本.txt")); c.Add(Path.Combine(root, "dnd") + Path.DirectorySeparatorChar); d.SetFileDropList(c); data = d; break; }
                case "delayed": data = new DataObject(new DelayedData()); break;
                default: return;
            }
            File.AppendAllText(log, DateTime.UtcNow.ToString("o") + " " + mode + " DoDragDrop start" + Environment.NewLine);
            var effect = form.DoDragDrop(data, DragDropEffects.Copy | DragDropEffects.Link);
            File.AppendAllText(log, DateTime.UtcNow.ToString("o") + " " + mode + " effect=" + effect + Environment.NewLine);
        };
        Application.Run(form);
    }
}
