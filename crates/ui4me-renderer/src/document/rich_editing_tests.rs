//! 1.0-track F1003: rich `contenteditable` hosts through the production facade (keys, IME,
//! clipboard, commands, undo, selection under mutation).
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

const STYLE: &str =
    "<style>body{margin:0;font:16px sans-serif}#ed{width:300px;min-height:40px}p{margin:0}</style>";

type Shared = Rc<RefCell<(String, Option<String>)>>;
struct HtmlClip(Shared);
impl Clipboard for HtmlClip {
    fn read_text(&mut self) -> std::result::Result<Option<String>, String> {
        Ok(Some(self.0.borrow().0.clone()))
    }
    fn write_text(&mut self, text: &str) -> std::result::Result<(), String> {
        *self.0.borrow_mut() = (text.into(), None);
        Ok(())
    }
    fn write_html(&mut self, html: &str, alt: &str) -> std::result::Result<(), String> {
        *self.0.borrow_mut() = (alt.into(), Some(html.into()));
        Ok(())
    }
    fn read_html(&mut self) -> std::result::Result<Option<String>, String> {
        Ok(self.0.borrow().1.clone())
    }
}

fn key(name: &str, ctrl: bool, shift: bool) -> Input {
    Input::Key {
        key: name.into(),
        code: name.into(),
        pressed: true,
        repeat: false,
        shift,
        ctrl,
        alt: false,
        meta: false,
    }
}
fn press(d: &mut Document, name: &str) -> Vec<Value> {
    d.handle_input(key(name, false, false)).unwrap()
}
fn ctrl(d: &mut Document, name: &str) -> Vec<Value> {
    d.handle_input(key(name, true, false)).unwrap()
}
fn type_text(d: &mut Document, t: &str) -> Vec<Value> {
    d.handle_input(Input::Text(t.into())).unwrap()
}
fn page(body: &str) -> (Document, Shared) {
    let shared: Shared = Rc::new(RefCell::new((String::new(), None)));
    let mut d = Document::parse(&format!("<!doctype html>{STYLE}{body}")).unwrap();
    d.set_clipboard(Box::new(HtmlClip(shared.clone())));
    d.render(400, 300, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    d.focus("ed").unwrap();
    (d, shared)
}
fn html(d: &Document) -> String {
    d.get_html(Some("ed"), None, false).unwrap()
}
fn text(d: &Document) -> String {
    d.get_text("ed").unwrap()
}
fn home(d: &mut Document) {
    ctrl(d, "Home");
}
fn end(d: &mut Document) {
    ctrl(d, "End");
}
fn right(d: &mut Document, n: usize, shift: bool) {
    for _ in 0..n {
        d.handle_input(key("ArrowRight", false, shift)).unwrap();
    }
}
fn state(d: &mut Document) -> Value {
    d.get_edit_state("ed").unwrap()
}

#[test]
fn typing_continues_nested_formatting_and_enter_backspace_split_and_merge() {
    let (mut d, _) = page("<div id=ed contenteditable><p>a<b>b<i>c</i></b></p></div>");
    end(&mut d);
    let ev = type_text(&mut d, "d");
    assert_eq!(html(&d), "<p>a<b>b<i>cd</i></b></p>");
    assert!(
        ev.iter()
            .any(|e| e["type"] == "input" && e["input_type"] == "insertText" && e["data"] == "d")
    );
    // Enter splits the paragraph inside the formatting; Backspace merges it again.
    press(&mut d, "ArrowLeft");
    let ev = press(&mut d, "Enter");
    assert_eq!(html(&d), "<p>a<b>b<i>c</i></b></p><p><b><i>d</i></b></p>");
    assert!(ev.iter().any(|e| e["input_type"] == "insertParagraph"));
    press(&mut d, "Backspace");
    assert_eq!(html(&d), "<p>a<b>b<i>cd</i></b></p>");
    assert_eq!(text(&d), "abcd");
    // Home/End and arrows work across paragraphs.
    home(&mut d);
    type_text(&mut d, ">");
    assert_eq!(html(&d), "<p>&gt;a<b>b<i>cd</i></b></p>");
}

#[test]
fn shift_enter_inserts_a_line_break_and_paragraph_deletes_merge() {
    let (mut d, _) = page("<div id=ed contenteditable><p>ab</p><p>cd</p></div>");
    home(&mut d);
    right(&mut d, 1, false);
    let ev = d.handle_input(key("Enter", false, true)).unwrap();
    assert_eq!(html(&d), "<p>a<br>b</p><p>cd</p>");
    assert!(ev.iter().any(|e| e["input_type"] == "insertLineBreak"));
    // Delete at a paragraph end merges the next one in.
    end(&mut d);
    right(&mut d, 0, false);
    home(&mut d);
    right(&mut d, 3, false); // a, <br>, b -> end of the first paragraph
    d.handle_input(key("Delete", false, false)).unwrap();
    assert_eq!(html(&d), "<p>a<br>bcd</p>");
    // Typing after the break lands on the second line of the same paragraph.
    home(&mut d);
    right(&mut d, 2, false);
    type_text(&mut d, "X");
    assert_eq!(html(&d), "<p>a<br>Xbcd</p>");
}

#[test]
fn ctrl_b_wraps_a_range_across_an_inline_boundary_and_toggles_back() {
    let (mut d, _) = page("<div id=ed contenteditable><p>ab<i>cd</i>ef</p></div>");
    home(&mut d);
    right(&mut d, 1, false);
    right(&mut d, 4, true); // "bcde"
    assert_eq!(state(&mut d)["bold"], false);
    let ev = ctrl(&mut d, "b");
    assert_eq!(
        html(&d),
        "<p>a<b>b</b><i><b>cd</b></i><b>e</b>f</p>",
        "bold wraps each text piece, italic stays"
    );
    assert!(ev.iter().any(|e| e["input_type"] == "formatBold"));
    assert_eq!(state(&mut d)["bold"], true);
    assert_eq!(state(&mut d)["italic"], false, "only part is italic");
    assert_eq!(text(&d), "abcdef");
    ctrl(&mut d, "b");
    assert_eq!(html(&d), "<p>ab<i>cd</i>ef</p>", "toggling back merges");
    // Italic and underline combine into nested markup.
    ctrl(&mut d, "i");
    assert_eq!(
        html(&d),
        "<p>a<i>bcde</i>f</p>",
        "partly italic: italic everywhere"
    );
    ctrl(&mut d, "u");
    assert_eq!(html(&d), "<p>a<i><u>bcde</u></i>f</p>");
}

#[test]
fn collapsed_caret_sets_a_pending_format_for_the_next_typed_text() {
    let (mut d, _) = page("<div id=ed contenteditable><p>ab</p></div>");
    end(&mut d);
    ctrl(&mut d, "b");
    assert_eq!(state(&mut d)["bold"], true, "pending format is reported");
    assert_eq!(html(&d), "<p>ab</p>", "nothing changes yet");
    type_text(&mut d, "X");
    assert_eq!(html(&d), "<p>ab<b>X</b></p>");
    type_text(&mut d, "Y");
    assert_eq!(
        html(&d),
        "<p>ab<b>XY</b></p>",
        "typing continues the format"
    );
    ctrl(&mut d, "b");
    type_text(&mut d, "Z");
    assert_eq!(html(&d), "<p>ab<b>XY</b>Z</p>");
    // Moving the caret drops the pending choice.
    ctrl(&mut d, "i");
    press(&mut d, "ArrowLeft");
    type_text(&mut d, "Q");
    assert!(!html(&d).contains("<i>"), "{}", html(&d));
}

#[test]
fn ime_composition_in_formatted_text_commits_once() {
    let (mut d, _) = page("<div id=ed contenteditable><p>a<b>b</b></p></div>");
    end(&mut d);
    d.handle_input(Input::ImePreedit {
        text: "に".into(),
        cursor: Some((3, 3)),
    })
    .unwrap();
    assert_eq!(html(&d), "<p>a<b>bに</b></p>");
    d.handle_input(Input::ImePreedit {
        text: "日本".into(),
        cursor: Some((6, 6)),
    })
    .unwrap();
    d.handle_input(Input::ImeCommit("日本".into())).unwrap();
    assert_eq!(html(&d), "<p>a<b>b日本</b></p>");
    assert_eq!(text(&d).matches("日本").count(), 1);
}

#[test]
fn pasted_hostile_html_is_reduced_to_the_allowed_subset() {
    let (mut d, clip) = page("<div id=ed contenteditable><p>start</p></div>");
    end(&mut d);
    *clip.borrow_mut() = (
        "plain".into(),
        Some(
            "<p onclick=x>Hi <b style=\"color:red\">there</b> <img src=x onerror=alert(1)>\
             <script>alert(1)</script><a href=\"javascript:alert(1)\">bad</a>\
             <a href=\"https://e.com/x\" onclick=y>good</a><span class=z>sp</span> \
             <table><tr><td>cell</td></tr></table></p><p><u>two</u></p>"
                .into(),
        ),
    );
    let ev = ctrl(&mut d, "v");
    assert!(ev.iter().any(|e| e["input_type"] == "insertFromPaste"));
    let h = html(&d);
    for bad in [
        "img",
        "script",
        "style",
        "onclick",
        "onerror",
        "javascript",
        "class",
        "table",
    ] {
        assert!(!h.contains(bad), "{bad} in {h}");
    }
    assert!(h.contains("<b>there</b>"), "{h}");
    assert!(h.contains("<a href=\"https://e.com/x\">good</a>"), "{h}");
    assert!(
        h.contains("bad") && h.contains("sp") && h.contains("<u>two</u>"),
        "{h}"
    );
    assert_eq!(h.matches("<p>").count(), 2, "{h}");
    assert!(d.query_ids("#ed img, #ed script", 10).unwrap().1 == 0);
    // Without clipboard HTML the text is inserted as text.
    *clip.borrow_mut() = ("<i>x</i>".into(), None);
    ctrl(&mut d, "v");
    assert!(text(&d).ends_with("<i>x</i>"));
    assert_eq!(d.query_ids("#ed i", 10).unwrap().1, 0);
}

#[test]
fn copy_and_cut_write_text_and_sanitized_html() {
    let (mut d, clip) = page("<div id=ed contenteditable><p>a<b>bc</b>d</p></div>");
    ctrl(&mut d, "a");
    ctrl(&mut d, "c");
    assert_eq!(clip.borrow().0, "abcd");
    assert_eq!(clip.borrow().1.as_deref(), Some("a<b>bc</b>d"));
    ctrl(&mut d, "x");
    assert_eq!(text(&d), "");
    ctrl(&mut d, "z");
    assert_eq!(html(&d), "<p>a<b>bc</b>d</p>");
}

#[test]
fn undo_and_redo_restore_markup_and_selection_for_each_edit() {
    let (mut d, _) = page("<div id=ed contenteditable><p>ab</p><p>cd</p></div>");
    let original = html(&d);
    home(&mut d);
    right(&mut d, 1, false);
    right(&mut d, 3, true); // "b", paragraph break, "c"
    ctrl(&mut d, "b");
    let bolded = html(&d);
    assert!(bolded.contains("<b>"), "{bolded}");
    type_text(&mut d, "X");
    assert_eq!(text(&d), "aXd");
    assert_eq!(html(&d).matches("<p>").count(), 1);
    let ev = ctrl(&mut d, "z");
    assert!(ev.iter().any(|e| e["input_type"] == "historyUndo"));
    assert_eq!(html(&d), bolded);
    ctrl(&mut d, "z");
    assert_eq!(html(&d), original);
    assert_eq!(d.selection_text().unwrap(), "b\nc", "selection restored");
    d.handle_input(key("y", true, false)).unwrap();
    assert_eq!(html(&d), bolded);
    d.handle_input(key("z", true, true)).unwrap();
    assert_eq!(text(&d), "aXd", "Ctrl+Shift+Z redoes");
    // Enter and Shift+Enter are undoable too.
    press(&mut d, "Enter");
    assert_eq!(html(&d).matches("<p>").count(), 2);
    ctrl(&mut d, "z");
    assert_eq!(html(&d).matches("<p>").count(), 1);
    // The history is bounded.
    for _ in 0..130 {
        type_text(&mut d, "k");
    }
    let mut steps = 0;
    while d.exec_edit("ed", "undo", None).unwrap() {
        steps += 1;
    }
    assert_eq!(steps, 100);
}

#[test]
fn exec_edit_and_get_edit_state_drive_the_same_commands() {
    let (mut d, _) = page("<div id=ed contenteditable><p>one two</p><p>three</p></div>");
    d.select_text(Some("ed")).unwrap();
    assert!(d.exec_edit("ed", "bold", None).unwrap());
    assert_eq!(html(&d), "<p><b>one two</b></p><p><b>three</b></p>");
    let st = state(&mut d);
    assert_eq!(st["bold"], true);
    assert_eq!(st["link"], Value::Null);
    assert!(d.exec_edit("ed", "italic", None).unwrap());
    assert!(d.exec_edit("ed", "strikethrough", None).unwrap());
    assert!(d.exec_edit("ed", "underline", None).unwrap());
    assert!(d.exec_edit("ed", "code", None).unwrap());
    let st = state(&mut d);
    assert_eq!(
        (
            st["italic"].as_bool(),
            st["strikethrough"].as_bool(),
            st["code"].as_bool()
        ),
        (Some(true), Some(true), Some(true))
    );
    assert!(d.exec_edit("ed", "remove_format", None).unwrap());
    assert_eq!(html(&d), "<p>one two</p><p>three</p>");
    // Links: only http/https/mailto, never activated, removable.
    d.select_text(Some("ed")).unwrap();
    assert!(
        d.exec_edit("ed", "link", Some("javascript:alert(1)"))
            .is_err()
    );
    assert!(d.exec_edit("ed", "link", Some("data:text/html,x")).is_err());
    assert!(
        d.exec_edit("ed", "link", Some("https://example.com/"))
            .unwrap()
    );
    assert!(
        html(&d).contains("<a href=\"https://example.com/\">one two</a>"),
        "{}",
        html(&d)
    );
    assert_eq!(state(&mut d)["link"], "https://example.com/");
    assert!(d.exec_edit("ed", "unlink", None).unwrap());
    assert_eq!(html(&d), "<p>one two</p><p>three</p>");
    // Text commands.
    d.select_text(Some("ed")).unwrap();
    assert!(d.exec_edit("ed", "insert_text", Some("hello")).unwrap());
    assert_eq!(html(&d), "<p>hello</p>");
    assert!(d.exec_edit("ed", "insert_paragraph", None).unwrap());
    assert!(
        d.exec_edit("ed", "insert_text", Some("world\nagain"))
            .unwrap()
    );
    assert_eq!(html(&d), "<p>hello</p><p>world</p><p>again</p>");
    assert!(d.exec_edit("ed", "undo", None).unwrap());
    assert!(d.exec_edit("ed", "undo", None).unwrap());
    assert_eq!(html(&d), "<p>hello</p>");
    assert!(d.exec_edit("ed", "redo", None).unwrap());
    assert!(d.exec_edit("ed", "nope", None).is_err());
    assert!(!d.exec_edit("ed", "redo", None).unwrap() || html(&d).contains("again"));
}

#[test]
fn exec_edit_requires_a_host_and_a_collapsed_format_is_pending() {
    let (mut d, _) = page("<div id=ed contenteditable>plain <i>x</i></div><p id=other>o</p>");
    assert!(d.exec_edit("other", "bold", None).is_err());
    assert!(d.get_edit_state("other").is_err());
    assert!(d.exec_edit("missing", "bold", None).is_err());
    // A first edit wraps loose inline content in a paragraph.
    end(&mut d);
    type_text(&mut d, "!");
    assert_eq!(html(&d), "<p>plain <i>x!</i></p>");
    assert!(d.exec_edit("ed", "bold", None).unwrap());
    assert_eq!(state(&mut d)["bold"], true);
    assert_eq!(state(&mut d)["italic"], true);
}

#[test]
fn selection_stays_valid_when_the_app_mutates_the_host() {
    let (mut d, _) =
        page("<div id=ed contenteditable><p id=p1>one</p><p id=p2>two</p><p id=p3>three</p></div>");
    end(&mut d);
    assert_eq!(d.selection_text().unwrap(), "");
    // The block holding the caret is removed.
    d.remove_element("p3").unwrap();
    d.render(400, 300, 1.0).unwrap();
    type_text(&mut d, "!");
    assert!(text(&d).contains('!'), "{}", html(&d));
    // The app rewrites the whole host, nested content included.
    d.replace_children_html("ed", "<p>new <b>bold</b></p>")
        .unwrap();
    d.render(400, 300, 1.0).unwrap();
    press(&mut d, "Backspace");
    type_text(&mut d, "Z");
    assert_eq!(text(&d).len(), "new bol".len() + 1);
    // A range selected, then both ends' blocks vanish.
    ctrl(&mut d, "a");
    d.replace_children_html("ed", "").unwrap();
    d.render(400, 300, 1.0).unwrap();
    ctrl(&mut d, "b");
    type_text(&mut d, "empty");
    assert_eq!(text(&d), "empty");
    assert_eq!(
        html(&d),
        "<p><b>empty</b></p>",
        "the pending format applies"
    );
    // Even a selection made before the content changed to something shorter stays editable.
    d.select_text(Some("ed")).unwrap();
    d.replace_children_html("ed", "<p>x</p>").unwrap();
    d.render(400, 300, 1.0).unwrap();
    type_text(&mut d, "y");
    assert!(text(&d).contains('y'));
}

#[test]
fn empty_hosts_blocks_and_keys_never_panic() {
    let (mut d, _) = page("<div id=ed contenteditable></div>");
    for k in ["Backspace", "Delete", "ArrowUp", "ArrowDown", "Home", "End"] {
        press(&mut d, k);
    }
    type_text(&mut d, "a");
    press(&mut d, "Enter");
    press(&mut d, "Enter");
    type_text(&mut d, "b");
    assert_eq!(html(&d), "<p>a</p><p><br></p><p>b</p>");
    press(&mut d, "ArrowUp");
    type_text(&mut d, "m");
    assert_eq!(html(&d), "<p>a</p><p>m</p><p>b</p>");
    d.handle_input(key("Enter", false, true)).unwrap();
    d.handle_input(key("Enter", false, true)).unwrap();
    assert_eq!(html(&d), "<p>a</p><p>m<br><br></p><p>b</p>");
}

#[test]
fn editing_creates_only_allowed_markup_and_ids_stay_unique() {
    let (mut d, _) = page("<div id=ed contenteditable><p id=one class=k>abc</p></div>");
    end(&mut d);
    press(&mut d, "Enter");
    type_text(&mut d, "<script>alert(1)</script>");
    let h = html(&d);
    assert!(!h.contains("<script"), "text is escaped: {h}");
    assert!(h.contains("&lt;script&gt;"), "{h}");
    assert_eq!(h.matches("id=\"one\"").count(), 1, "{h}");
    assert_eq!(h.matches("class=\"k\"").count(), 2, "{h}");
    ctrl(&mut d, "a");
    ctrl(&mut d, "b");
    ctrl(&mut d, "u");
    let h = html(&d);
    assert!(!h.contains("style") && !h.contains(" on"), "{h}");
}

#[test]
fn plaintext_only_hosts_keep_the_plain_behaviour() {
    let (mut d, clip) = page("<div id=ed contenteditable=plaintext-only>Ahoj <b>svet</b></div>");
    end(&mut d);
    type_text(&mut d, "!");
    assert_eq!(html(&d), "Ahoj svet!", "flattened to one text node");
    ctrl(&mut d, "b");
    assert_eq!(html(&d), "Ahoj svet!", "no formatting in plain hosts");
    *clip.borrow_mut() = ("t".into(), Some("<b>x</b>".into()));
    ctrl(&mut d, "v");
    assert_eq!(html(&d), "Ahoj svet!t", "HTML clipboard is pasted as text");
    assert!(!d.exec_edit("ed", "bold", None).unwrap());
    assert_eq!(state(&mut d)["bold"], false);
    assert!(d.exec_edit("ed", "insert_text", Some("?")).unwrap());
    assert!(d.exec_edit("ed", "undo", None).unwrap());
    assert_eq!(html(&d), "Ahoj svet!t");
}

#[test]
fn rich_hosts_stay_multiline_text_fields_whose_value_is_the_text() {
    let (mut d, _) =
        page("<div id=ed contenteditable aria-label=Notes><p>Hello <b>big</b></p></div>");
    let t = d.accessibility_tree().unwrap();
    let n = t
        .nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some("ed"))
        .unwrap();
    assert_eq!(n.role, accesskit::Role::MultilineTextInput);
    assert!(
        n.value.as_deref().unwrap_or_default().contains("Hello big"),
        "{:?}",
        n.value
    );
}

#[test]
fn model_round_trips_unknown_markup_and_reports_flat_offsets() {
    use super::rich_edit::Rich;
    let (d, _) = page(
        "<div id=ed contenteditable><p>a<span class=s>b<b>c</b></span><img src=x></p><hr><table><tr><td>t</td></tr></table></div>",
    );
    let host = d.node("ed").unwrap();
    let r = d.rich_load(host);
    assert_eq!(r.text(), "abc");
    let h = r.html();
    assert!(h.contains("<span class=\"s\">b<b>c</b></span>"), "{h}");
    assert!(
        h.contains("<img src=\"x\">") && h.contains("<hr>") && h.contains("<table>"),
        "{h}"
    );
    let (b, off) = r.locate(2);
    assert_eq!((b, off), (0, 2));
    assert_eq!(r.flat_of(0, 3), 3);
    let _ = Rich::paste_paragraphs("<p>x</p>").unwrap();
}

#[test]
fn exec_edit_formats_a_selection_made_without_focus() {
    // A host program selects and formats before (or without) the window having OS focus.
    let mut d =
        Document::parse("<!doctype html><div id=ed contenteditable>hello world</div>").unwrap();
    d.select_text(Some("ed")).unwrap();
    // Loose host text is one synthetic paragraph; formatting it was a no-op that reported success.
    assert!(d.exec_edit("ed", "bold", None).unwrap());
    assert_eq!(html(&d), "<p><b>hello world</b></p>");
    assert_eq!(d.get_edit_state("ed").unwrap()["bold"], true);
    d.select_text(Some("ed")).unwrap();
    assert!(d.exec_edit("ed", "italic", None).unwrap());
    assert_eq!(html(&d), "<p><b><i>hello world</i></b></p>");
}
