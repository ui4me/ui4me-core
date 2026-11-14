"""A small notes app: HTML and CSS files, a form, a list and native dialogs.

Run it:  python app.py      (or: uv run app.py, from this folder)
"""
from html import escape
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Notes", app_id="org.ui4me.PythonNotes") as app:
        window = app.open_file(HERE / "index.html", title="Notes", width=720, height=520)
        document = window.document
        notes = []

        def show_notes():
            items = "".join(f"<li>{escape(note)}</li>" for note in notes)
            document.replace_html("notes", items or '<li class="empty">No notes yet.</li>')
            document.set_text("count", f"{len(notes)} note(s)")

        def add_note(event):
            text = document.get_value("note").strip()
            if not text:
                document.set_text("status", "Write something first.")
                return
            notes.append(text)
            document.set_value("note", "")
            document.set_text("status", "Added.")
            show_notes()

        def clear_notes(event):
            notes.clear()
            document.set_text("status", "Cleared.")
            show_notes()

        def export_notes(event):
            # The callback may wait for the native dialog; the window stays responsive.
            result = window.save_file(file_name="notes.txt", title="Export notes")
            if result["cancelled"] or not result["paths"]:
                document.set_text("status", "Export cancelled.")
                return
            path = Path(result["paths"][0])
            path.write_text("\n".join(notes) + "\n", encoding="utf-8")
            document.set_text("status", f"Exported to {path}")

        document.on("click", add_note, element="add")
        document.on("click", clear_notes, element="clear")
        document.on("click", export_notes, element="export")
        show_notes()
        app.run()


if __name__ == "__main__":
    main()
