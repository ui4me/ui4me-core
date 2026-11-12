//! owner: 003-document-model
//!
//! T039 / T076: the 012 hooks (FR-099) and the 007 notifications (FR-100) for single calls and
//! batches: exactly the specified element and attribute set, old and new values, hooks inside the
//! call, no calls for failed calls or template contents, staged-then-released notifications, and
//! hook writes undone on batch rollback.

use std::cell::RefCell;
use std::rc::Rc;
use ui4me_document::batch::decode::{Cmd, Encoder, Operand};
use ui4me_document::batch::exec::{document_word, node_word};
use ui4me_document::document::Document;
use ui4me_document::hooks::{Notification, RequesterChange, RequesterKind};
use ui4me_document::ui_state::UiFlags;

#[derive(Debug, Clone, PartialEq)]
enum Ev {
    Attr(String, Option<String>, Option<String>),
    Inserted,
    Removed,
    Children(String),
}

type Log = Rc<RefCell<Vec<Ev>>>;
type Notes = Rc<RefCell<Vec<Notification>>>;

fn wired(html: &str) -> (Document, Log, Notes) {
    let mut d = Document::parse_html(html).unwrap();
    let log: Log = Rc::default();
    let notes: Notes = Rc::default();
    let l = log.clone();
    d.hooks_mut()
        .set_on_attribute_changed(Box::new(move |_w, c| {
            l.borrow_mut()
                .push(Ev::Attr(c.local_name.clone(), c.old.clone(), c.new.clone()))
        }));
    let l = log.clone();
    d.hooks_mut()
        .set_on_inserted(Box::new(move |_w, _n| l.borrow_mut().push(Ev::Inserted)));
    let l = log.clone();
    d.hooks_mut()
        .set_on_removed(Box::new(move |_w, _r| l.borrow_mut().push(Ev::Removed)));
    let l = log.clone();
    d.hooks_mut().set_on_children_changed(Box::new(move |w, p| {
        l.borrow_mut()
            .push(Ev::Children(w.document().local_name(p).unwrap().to_owned()))
    }));
    let n = notes.clone();
    d.hooks_mut()
        .set_notification_sink(Box::new(move |x| n.borrow_mut().push(x.clone())));
    (d, log, notes)
}

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap()
}

// table-a: A-43
#[test]
fn the_attribute_hook_fires_for_exactly_the_fr099_set_with_old_and_new_values() {
    let (mut d, log, _) =
        wired("<input id=i><div id=d><span id=s></span></div><details id=x></details><p id=p></p>");
    let (i, dv, s, x, p) = (
        id(&d, "i"),
        id(&d, "d"),
        id(&d, "s"),
        id(&d, "x"),
        id(&d, "p"),
    );
    d.set_attribute(i, "value", "1").unwrap();
    d.set_attribute(i, "value", "2").unwrap();
    d.remove_attribute(i, "value").unwrap();
    d.set_attribute(x, "open", "").unwrap();
    // generic attributes on any element: only tabindex, disabled, inert, hidden
    d.set_attribute(dv, "class", "c").unwrap();
    d.set_attribute(s, "title", "t").unwrap();
    d.set_attribute(dv, "tabindex", "0").unwrap();
    d.set_attribute(s, "disabled", "").unwrap();
    d.toggle_attribute(p, "hidden", None).unwrap();
    d.set_attribute(p, "inert", "").unwrap();
    // unchanged writes do not call the hook
    d.set_attribute(p, "inert", "").unwrap();
    assert_eq!(
        *log.borrow(),
        vec![
            Ev::Attr("value".into(), None, Some("1".into())),
            Ev::Attr("value".into(), Some("1".into()), Some("2".into())),
            Ev::Attr("value".into(), Some("2".into()), None),
            Ev::Attr("open".into(), None, Some("".into())),
            Ev::Attr("tabindex".into(), None, Some("0".into())),
            Ev::Attr("disabled".into(), None, Some("".into())),
            Ev::Attr("hidden".into(), None, Some("".into())),
            Ev::Attr("inert".into(), None, Some("".into())),
        ]
    );
}

// table-a: A-27
#[test]
fn insertion_removal_and_children_hooks_run_inside_the_call_and_not_for_failed_calls() {
    let (mut d, log, _) = wired("<div id=a><select id=s><option id=o>1</select></div>");
    let (a, s, o) = (id(&d, "a"), id(&d, "s"), id(&d, "o"));
    let rev = d.revision();
    let n = d.create_element("p").unwrap();
    d.append_child(a, n).unwrap();
    assert_eq!(*log.borrow(), vec![Ev::Inserted]);
    log.borrow_mut().clear();
    d.remove_child(a, n).unwrap();
    assert_eq!(*log.borrow(), vec![Ev::Removed]);
    log.borrow_mut().clear();
    // the select and its option parent get the children hook
    let o2 = d.create_element("option").unwrap();
    d.append_child(s, o2).unwrap();
    assert_eq!(
        *log.borrow(),
        vec![Ev::Inserted, Ev::Children("select".into())]
    );
    log.borrow_mut().clear();
    d.remove_child(s, o).unwrap();
    assert!(log.borrow().contains(&Ev::Children("select".into())));
    log.borrow_mut().clear();
    // failed call: nothing
    let _ = d.append_child(n, a); // cycle? n is detached: a is an ancestor of nothing here; use a real failure
    log.borrow_mut().clear();
    let before = d.revision();
    assert!(d.append_child(a, a).is_err());
    assert!(log.borrow().is_empty(), "never called for a failed call");
    assert_eq!(d.revision(), before);
    assert!(before > rev);
}

// table-a: A-52
#[test]
fn template_contents_are_inert_for_hooks_and_notifications() {
    let (mut d, log, notes) = wired("<template id=t></template><div id=a></div>");
    let t = id(&d, "t");
    let c = d.template_contents(t).unwrap().unwrap();
    let img = d.create_element("img").unwrap();
    d.set_attribute(img, "src", "x.png").unwrap();
    log.borrow_mut().clear();
    notes.borrow_mut().clear();
    d.append_child(c, img).unwrap();
    d.set_attribute(img, "src", "y.png").unwrap();
    d.set_attribute(img, "disabled", "").unwrap();
    assert!(
        log.borrow().is_empty(),
        "no hook calls for nodes in template contents: {:?}",
        log.borrow()
    );
    assert!(
        notes.borrow().is_empty(),
        "no requester notification for elements inside contents"
    );
    // moving it out into the document connects it
    let a = id(&d, "a");
    d.append_child(a, img).unwrap();
    assert!(notes.borrow().iter().any(|n| matches!(
        n,
        Notification::RequesterChanged {
            kind: RequesterKind::Img,
            change: RequesterChange::Connected,
            ..
        }
    )));
}

// table-a: A-27
#[test]
fn requester_notifications_for_connect_disconnect_and_attribute_changes() {
    let (mut d, _, notes) = wired("<div id=a></div>");
    let a = id(&d, "a");
    let img = d.create_element("img").unwrap();
    d.append_child(a, img).unwrap();
    d.set_attribute(img, "src", "1.png").unwrap();
    d.set_attribute(img, "alt", "no request").unwrap();
    d.remove_child(a, img).unwrap();
    let kinds: Vec<_> = notes
        .borrow()
        .iter()
        .map(|n| match n {
            Notification::RequesterChanged { change, .. } => format!("{change:?}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        ["Connected", "AttributeChanged(\"src\")", "Disconnected"]
    );
    notes.borrow_mut().clear();
    // link rel=stylesheet and base
    let l = d.create_element("link").unwrap();
    d.set_attribute(l, "rel", "stylesheet").unwrap();
    d.set_attribute(l, "href", "a.css").unwrap();
    d.append_child(a, l).unwrap();
    assert!(notes.borrow().iter().any(
        |n| matches!(n, Notification::StylesheetLinkChanged { href: Some(h), .. } if h == "a.css")
    ));
    notes.borrow_mut().clear();
    let b = d.create_element("base").unwrap();
    d.set_attribute(b, "href", "http://h/").unwrap();
    let head = d.head().unwrap();
    d.append_child(head, b).unwrap();
    assert!(notes.borrow().contains(&Notification::BaseUrlChanged {
        url: Some("http://h/".into())
    }));
}

fn word(d: &Document, n: ui4me_engine_types::ids::NodeId) -> Operand {
    Operand::Existing(document_word(d.id()), node_word(n))
}

fn enc(cmds: &[Cmd]) -> Vec<u8> {
    let mut e = Encoder::new();
    for c in cmds {
        e.push(c);
    }
    e.finish()
}

// table-a: A-62
#[test]
fn batch_hooks_run_inline_notifications_are_released_once_on_commit_in_command_order() {
    let (mut d, log, notes) = wired("<div id=a></div>");
    let a = word(&d, id(&d, "a"));
    let cmds = vec![
        Cmd::CreateElement {
            ns: None,
            qname: "img".into(),
        },
        Cmd::SetAttribute {
            el: Operand::Local(0),
            ns: None,
            qname: "src".into(),
            value: "1.png".into(),
        },
        Cmd::AppendChild {
            parent: a,
            child: Operand::Local(0),
        },
        Cmd::SetAttribute {
            el: Operand::Local(0),
            ns: None,
            qname: "src".into(),
            value: "2.png".into(),
        },
        Cmd::SetAttribute {
            el: a,
            ns: None,
            qname: "tabindex".into(),
            value: "0".into(),
        },
    ];
    d.batch_apply(&enc(&cmds)).unwrap();
    // `src` on `img` is not in the FR-099 hook set; insertion and `tabindex` are.
    assert_eq!(
        *log.borrow(),
        vec![
            Ev::Inserted,
            Ev::Attr("tabindex".into(), None, Some("0".into()))
        ]
    );
    let kinds: Vec<_> = notes.borrow().iter().map(|n| format!("{n:?}")).collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds[0].contains("Connected") && kinds[1].contains("AttributeChanged"));
}

// table-a: A-62
#[test]
fn batch_rollback_undoes_hook_ui_writes_and_drops_staged_notifications() {
    let (mut d, _log, notes) = wired("<div id=a></div><input id=i>");
    // a hook that writes UI state on every `disabled` attribute change
    d.hooks_mut().set_on_attribute_changed(Box::new(|w, c| {
        if c.local_name == "disabled" {
            w.set_flags(c.element, UiFlags::FOCUS).unwrap();
        }
    }));
    let (a, i) = (id(&d, "a"), id(&d, "i"));
    let before_ui = d.ui_flags(i).unwrap();
    let rev = d.revision();
    let seg_len = d.journal().since(0).entries.len();
    let cmds = vec![
        Cmd::CreateElement {
            ns: None,
            qname: "img".into(),
        },
        Cmd::AppendChild {
            parent: word(&d, a),
            child: Operand::Local(0),
        },
        Cmd::SetAttribute {
            el: word(&d, i),
            ns: None,
            qname: "disabled".into(),
            value: "".into(),
        },
        Cmd::SetAttribute {
            el: word(&d, i),
            ns: None,
            qname: "bad name".into(),
            value: "".into(),
        },
    ];
    let e = d.batch_apply(&enc(&cmds)).unwrap_err();
    assert_eq!(e.batch_index, Some(3));
    assert_eq!(
        d.ui_flags(i).unwrap(),
        before_ui,
        "the hook's UI-state write was undone"
    );
    assert!(!d.has_attribute(i, "disabled").unwrap());
    assert!(
        notes.borrow().is_empty(),
        "no notification after a rollback"
    );
    assert_eq!(d.revision(), rev);
    assert_eq!(d.journal().since(0).entries.len(), seg_len);
    // and the same commands without the failure commit with the UI write intact
    let ok = enc(&cmds[..3]);
    d.batch_apply(&ok).unwrap();
    assert_eq!(d.ui_flags(i).unwrap(), UiFlags::FOCUS);
    assert_eq!(notes.borrow().len(), 1);
}
