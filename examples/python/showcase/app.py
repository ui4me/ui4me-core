"""UI4ME Showcase: a multi-page tour of the HTML and CSS that UI4ME Core supports.

Run it:  python app.py      (or: uv run app.py, from this folder)

The interface is ui/index.html and ui/style.css. This script is the backend:
it switches pages, toggles the theme, reads the form, opens the dialog,
shows toasts and reacts to links. Copy the folder as a starting point.
"""
import platform
from html import escape
from pathlib import Path

from ui4me_core import Application

UI = Path(__file__).resolve().parent / "ui"

# Page name -> (title, subtitle). Each page is a <section id="page-NAME">,
# and its sidebar button is <button id="nav-NAME">.
PAGES = {
    "overview": ("Overview", "Cards, stat tiles, badges, progress bars and a table."),
    "forms": ("Forms", "Inputs, selects, checkboxes, radios, validation and submit."),
    "layouts": ("Layouts", "Flexbox, grid areas, a holy-grail layout and sticky headings."),
    "content": ("Typography & content", "Headings, lists, quotes, code, links and an accordion."),
    "overlays": ("Feedback & overlays", "Dialog, popover, toasts, a counter and the theme."),
}

# Labels for the validation message, by input id.
FIELD_LABELS = {
    "full-name": "Full name", "email": "Email", "username": "Username",
    "password": "Password", "website": "Website", "team-size": "Team size", "terms": "Terms",
}
TAKEN_USERNAMES = {"admin", "root", "ui4me"}
TOASTS = {
    "success": ("Saved", "Everything went well."),
    "warning": ("Check this", "Something needs your attention."),
    "error": ("Failed", "The action could not be completed."),
}


class Showcase:
    """Application state plus one named method per event handler."""

    def __init__(self, app, window):
        self.app = app
        self.window = window
        self.document = None
        self.page = "overview"
        self.theme = "system"  # "system", "light" or "dark"
        self.counter = 0
        self.progress = 64
        self.toast_count = 0
        self.invalid_fields = []

    # ----- Loading documents ------------------------------------------------

    def open_main(self):
        """Load index.html and connect every handler (also used by 'Back')."""
        self.document = self.window.load_file(UI / "index.html")
        d = self.document
        # A form submit would otherwise be a native action; prevent it up front.
        d.prevent_default("signup", "submit")

        d.on("click", self.on_navigate, element="nav")
        d.on("click", self.on_go_forms, element="go-forms")
        d.on("click", self.on_theme_toggle, element="theme-toggle")
        d.on("change", self.on_theme_choice, element="theme-choice")
        d.on("environment_changed", self.on_environment_changed)
        d.on("link_activated", self.on_link)
        d.on("click", self.on_advance, element="advance")
        # Forms page
        d.on("submit", self.on_submit, element="signup")
        d.on("invalid", self.on_invalid)  # invalid does not bubble: no element filter
        d.on("input", self.on_form_input, element="signup")
        d.on("reset", self.on_form_reset, element="signup")
        # Overlays page
        d.on("click", self.on_open_dialog, element="open-dialog")
        d.on("click", self.on_dialog_button, element="demo-dialog")
        d.on("close", self.on_dialog_closed, element="demo-dialog")
        d.on("click", self.on_toggle_popover, element="toggle-popover")
        d.on("toggle", self.on_popover_toggled, element="tips-popover")
        d.on("click", self.on_counter, element="counter-controls")
        d.on("click", self.on_counter, element="counter-reset")
        d.on("click", self.on_toast_button, element="toast-buttons")
        d.on("click", self.on_toast_close, element="toasts")

        # Restore what the backend remembers (matters after returning from about.html).
        d.set_text("backend-name", f"Python {platform.python_version()} on {platform.system()}")
        self.apply_theme()
        self.show_page(self.page)
        self.show_counter()
        self.show_progress()

    def open_about(self):
        """Load a second HTML file into the same window."""
        self.document = self.window.load_file(UI / "about.html")
        self.document.on("link_activated", self.on_link)
        self.apply_theme()

    # ----- Helpers ----------------------------------------------------------

    def set_status(self, text):
        self.document.set_text("status", text)

    def show_page(self, name):
        """Show one <section class="page"> and hide the others, in one atomic batch."""
        if name not in PAGES:
            return
        self.page = name
        title, subtitle = PAGES[name]
        operations = []
        for page in PAGES:
            change = "add_class" if page == name else "remove_class"
            current = "page" if page == name else "false"
            operations += [
                {"op": change, "element": f"page-{page}", "name": "active"},
                {"op": change, "element": f"nav-{page}", "name": "active"},
                {"op": "set_attribute", "element": f"nav-{page}", "name": "aria-current", "value": current},
            ]
        operations += [
            {"op": "set_text", "element": "page-title", "text": title},
            {"op": "set_text", "element": "page-subtitle", "text": subtitle},
            {"op": "set_text", "element": "status-page", "text": title},
        ]
        self.document.batch(operations)
        self.document.scroll_to(element="pages", y=0)

    def effective_scheme(self):
        """'light' or 'dark': the forced theme, or what the desktop prefers."""
        if self.theme != "system":
            return self.theme
        return self.document.environment()["color_scheme"]

    def apply_theme(self):
        d = self.document
        # style.css: .theme-light / .theme-dark force a color-scheme; theme-system follows the desktop.
        d.set_attribute("app", "class", "theme-" + self.theme)
        if self.page_has("theme-toggle"):
            scheme = self.effective_scheme()
            d.set_checked("theme-mode-" + self.theme, True)
            d.set_text("theme-label", "Light mode" if scheme == "dark" else "Dark mode")
            d.set_text("stat-theme", scheme.capitalize())
            d.set_text("stat-theme-source", "Follows the desktop" if self.theme == "system" else "Chosen in the app")

    def page_has(self, element):
        return bool(self.document.query("#" + element)["total"])

    def show_counter(self):
        value = str(self.counter)
        self.document.set_text("counter-value", value)
        self.document.set_text("stat-counter", value)
        if self.counter < 0:
            self.document.add_class("counter-value", "negative")
        else:
            self.document.remove_class("counter-value", "negative")

    def show_progress(self):
        self.document.set_attribute("build-progress", "value", str(self.progress))
        self.document.set_text("progress-label", f"{self.progress}%")
        self.document.set_text("stat-progress", f"{self.progress}%")

    # ----- Navigation, theme and links --------------------------------------

    def on_navigate(self, event):
        # One handler for the whole <nav>: event["element"] is the button's id.
        if event["element"].startswith("nav-"):
            self.show_page(event["element"].removeprefix("nav-"))
            self.set_status(f"Showing the {PAGES[self.page][0]} page.")

    def on_go_forms(self, event):
        self.show_page("forms")
        self.document.focus("full-name")

    def on_theme_toggle(self, event):
        self.theme = "dark" if self.effective_scheme() == "light" else "light"
        self.apply_theme()
        self.set_status(f"Theme set to {self.theme}.")

    def on_theme_choice(self, event):
        if event["element"].startswith("theme-mode-") and event.get("checked"):
            self.theme = event["element"].removeprefix("theme-mode-")
            self.apply_theme()
            self.set_status(f"Theme: {self.theme}.")

    def on_environment_changed(self, event):
        # The desktop switched between light and dark.
        self.apply_theme()

    def on_link(self, event):
        href = event.get("href", "")
        if href.startswith("#"):
            self.show_page(href[1:])
            self.set_status(f"Link to {href} handled by the backend.")
        elif href == "about.html":
            self.open_about()
        elif href == "index.html":
            self.open_main()
            self.set_status("Back from about.html: the page, theme and counter were restored.")
        else:
            # A real application could call self.app.open_external(event["url"]).
            self.set_status(f"Link activated: {event['url']} (not opened in this demo).")

    def on_advance(self, event):
        self.progress = 0 if self.progress >= 100 else min(100, self.progress + 12)
        self.show_progress()
        self.set_status(f"Build progress: {self.progress}%.")

    # ----- Forms --------------------------------------------------------------

    def on_form_input(self, event):
        d = self.document
        self.invalid_fields.clear()
        if event["element"] == "volume":
            d.set_text("volume-out", event.get("value", ""))
        elif event["element"] == "username":
            if event.get("value", "").lower() in TAKEN_USERNAMES:
                d.set_custom_validity("username", "That username is already taken.")
                d.set_text("username-hint", "That username is already taken.")
                d.add_class("username-hint", "error")
            else:
                d.set_custom_validity("username", "")
                d.set_text("username-hint", 'Try "admin": the backend marks it as taken.')
                d.remove_class("username-hint", "error")

    def on_invalid(self, event):
        # One event per invalid control, sent when a submit attempt fails validation.
        label = FIELD_LABELS.get(event["element"], event["element"])
        if label not in self.invalid_fields:
            self.invalid_fields.append(label)
        d = self.document
        d.add_class("signup", "show-errors")
        d.set_attribute("form-feedback", "class", "feedback error")
        d.set_text("form-feedback", "Please check: " + ", ".join(self.invalid_fields) + ".")

    def on_submit(self, event):
        d = self.document
        self.invalid_fields.clear()
        # Entries arrive in document order; repeated names (checkboxes, multi-select) repeat.
        values = {}
        for entry in d.form_data("signup")["entries"]:
            value = "*" * len(entry["value"]) if entry["name"] == "password" else entry["value"]
            values.setdefault(entry["name"], []).append(value)
        rows = "".join(
            f"<dt>{escape(name)}</dt><dd>{escape(', '.join(items) or '-')}</dd>"
            for name, items in values.items())
        d.replace_html("form-result", rows)
        d.set_text("result-badge", f"{len(values)} fields")
        d.set_attribute("result-badge", "class", "badge success")
        d.remove_class("signup", "show-errors")
        d.set_attribute("form-feedback", "class", "feedback success")
        d.set_text("form-feedback", "Submitted. The backend read the form with form_data().")
        self.set_status("Form submitted.")

    def on_form_reset(self, event):
        d = self.document
        d.set_custom_validity("username", "")
        d.set_text("volume-out", d.get_value("volume"))
        d.remove_class("signup", "show-errors")
        d.set_attribute("form-feedback", "class", "feedback")
        d.set_text("form-feedback", "The form was reset to its default values.")
        self.set_status("Form reset.")

    # ----- Overlays -----------------------------------------------------------

    def on_open_dialog(self, event):
        self.document.show_dialog("demo-dialog", modal=True)

    def on_dialog_button(self, event):
        if event["element"] == "dialog-confirm":
            self.document.close_dialog("demo-dialog", "published")
        elif event["element"] == "dialog-cancel":
            self.document.close_dialog("demo-dialog", "cancelled")

    def on_dialog_closed(self, event):
        answer = event.get("return_value") or "dismissed"
        if answer == "published":
            note = self.document.get_value("dialog-note")
            text = f'Published with the note "{note}".'
        else:
            text = f"Dialog {answer}."
        self.document.set_text("dialog-result", text)
        self.set_status(text)

    def on_toggle_popover(self, event):
        # Returns the new state; the "toggle" event below reports it as well.
        self.document.toggle_popover("tips-popover")

    def on_popover_toggled(self, event):
        self.set_status(f"The popover is now {event.get('new_state', 'toggled')}.")

    def on_counter(self, event):
        changes = {"counter-inc": 1, "counter-dec": -1}
        if event["element"] == "counter-reset":
            self.counter = 0
        elif event["element"] in changes:
            self.counter += changes[event["element"]]
        else:
            return
        self.show_counter()

    def on_toast_button(self, event):
        kind = event["element"].removeprefix("toast-")
        if kind == "clear":
            self.document.replace_html("toasts", "")
            return
        if kind not in TOASTS:
            return
        self.toast_count += 1
        number = self.toast_count
        title, text = TOASTS[kind]
        toast = (f'<div id="toast-{number}" class="toast {kind}" role="status">'
                 f'<div class="toast-body"><strong>{title}</strong><span>{text} (#{number})</span></div>'
                 f'<button id="toast-close-{number}" class="toast-close" type="button" aria-label="Dismiss">'
                 f'&times;</button></div>')
        # Add the new toast and keep only the newest three, in one transaction.
        self.document.batch([
            {"op": "insert_html", "element": "toasts", "position": "beforeend", "html": toast},
            {"op": "trim_children", "element": "toasts", "keep": 3, "from": "start"},
        ])

    def on_toast_close(self, event):
        if event["element"].startswith("toast-close-"):
            self.document.remove("toast-" + event["element"].removeprefix("toast-close-"))


def main():
    with Application.start(name="UI4ME Showcase", app_id="org.ui4me.examples.Showcase") as app:
        window = app.create_window(title="UI4ME Showcase", width=1100, height=760,
                                   min_width=640, min_height=480)
        showcase = Showcase(app, window)
        showcase.open_main()
        app.run()


if __name__ == "__main__":
    main()
