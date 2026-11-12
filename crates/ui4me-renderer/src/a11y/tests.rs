//! Mapping tests on representative markup (DOM only: a stub [`RenderInfo`] answers "rendered"
//! except for elements carrying `data-display-none` / `data-visibility-hidden`, standing in for
//! the committed style the runtime supplies). The runtime's integration test
//! (`ui4me-runtime/tests/accessibility_tree.rs`) runs the same mapping on real committed style.

use super::*;
use ui4me_document::document::Document;

struct Stub<'a>(&'a Document);

impl RenderInfo for Stub<'_> {
    fn rendering(&self, element: NodeId) -> Rendering {
        let mut n = Some(element);
        while let Some(x) = n {
            let e = self.0.arena().get(x).and_then(|y| y.as_element());
            if e.is_some_and(|e| e.attr("data-display-none").is_some()) {
                return Rendering::NotRendered;
            }
            n = self.0.arena().get(x).and_then(|y| y.parent());
        }
        let e = self.0.arena().get(element).and_then(|y| y.as_element());
        if e.is_some_and(|e| e.attr("data-visibility-hidden").is_some()) {
            return Rendering::Invisible;
        }
        Rendering::Rendered
    }
    fn bounds(&self, _node: NodeId) -> Option<Bounds> {
        Some(Bounds {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        })
    }
}

fn tree(html: &str) -> A11yTree {
    tree_focused(html, None)
}

fn tree_focused(html: &str, focus_id: Option<&str>) -> A11yTree {
    let d = Document::parse_html(html).expect("parse");
    let focused = focus_id.and_then(|i| d.element_by_id(i));
    let t = build(&BuildInput {
        document: &d,
        info: &Stub(&d),
        focused,
        title: "Win",
        viewport: (800.0, 600.0),
        scale: 1.25,
    });
    t.validate()
        .unwrap_or_else(|e| panic!("invalid tree: {e}\n{}", t.to_json()));
    t
}

fn by_html_id<'a>(t: &'a A11yTree, id: &str) -> &'a A11yNode {
    t.in_order()
        .into_iter()
        .find(|n| n.html_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no node #{id} in {}", t.to_json()))
}

const FORM: &str = r##"<!doctype html><html><head><title>Admin</title></head><body>
<nav class="navbar" aria-label="Main"><a class="navbar-brand" href="#">Dashboard</a></nav>
<main>
<h1>Users <small>admin</small></h1>
<form class="needs-validation">
  <div class="mb-3"><label for="email" class="form-label">Email address</label>
    <input type="email" class="form-control" id="email" placeholder="name@example.com" required></div>
  <div class="mb-3"><label for="pw">Password</label><input type="password" id="pw" value="secret"></div>
  <div class="form-check"><input class="form-check-input" type="checkbox" id="remember" checked>
    <label class="form-check-label" for="remember">Remember me</label></div>
  <label class="form-check"><input type="radio" name="r" id="r1"> Option one</label>
  <select id="role" class="form-select" aria-label="Role"><option>Viewer</option><option selected>Editor</option></select>
  <input type="range" id="vol" min="0" max="10" value="4" aria-label="Volume">
  <textarea id="notes" placeholder="Notes"></textarea>
  <input type="text" id="ro" readonly value="fixed" title="Read only field">
  <input type="text" id="bad" aria-invalid="true" aria-labelledby="badlbl"><span id="badlbl">Broken <b>field</b></span>
  <button type="submit" class="btn btn-primary" id="save">Save changes</button>
  <button type="button" class="btn-close" id="close" aria-label="Close"><svg aria-hidden="true"><path d="M0 0"/></svg></button>
  <button type="button" id="icon"><i class="bi bi-trash" aria-hidden="true"></i> Delete</button>
  <input type="submit" id="go">
  <button id="dis" disabled>Disabled</button>
  <button id="tog" aria-pressed="true" aria-expanded="false">Menu</button>
</form>
<progress id="prog" value="30" max="100" aria-label="Upload"></progress>
</main></body></html>"##;

#[test]
fn bootstrap_form_roles_names_and_states() {
    let t = tree_focused(FORM, Some("email"));
    let root = t.get(ROOT_ID).unwrap();
    assert_eq!((root.role, root.name.as_str()), (Role::Window, "Win"));
    let doc = t.get(root.children[0]).unwrap();
    assert_eq!((doc.role, doc.name.as_str()), (Role::RootWebArea, "Admin"));

    let nav = t
        .in_order()
        .into_iter()
        .find(|n| n.role == Role::Navigation)
        .unwrap();
    assert_eq!(nav.name, "Main");
    let link = t.find(Role::Link, "Dashboard").expect("link");
    assert!(link.actions.contains(&A11yAction::Click) && link.actions.contains(&A11yAction::Focus));
    assert!(link.children.is_empty(), "text child folded into the name");

    let h1 = t
        .in_order()
        .into_iter()
        .find(|n| n.role == Role::Heading)
        .unwrap();
    assert_eq!((h1.name.as_str(), h1.level), ("Users admin", Some(1)));

    let email = by_html_id(&t, "email");
    assert_eq!(
        (email.role, email.name.as_str()),
        (Role::EmailInput, "Email address")
    );
    assert_eq!(email.placeholder.as_deref(), Some("name@example.com"));
    assert_eq!(email.value.as_deref(), Some(""));
    assert!(email.states.required && email.states.focused && email.states.focusable);
    assert!(email.actions.contains(&A11yAction::SetValue));
    assert_eq!(t.focus, email.id);
    assert_eq!(email.children.len(), 1, "text run");
    assert_eq!(t.get(email.children[0]).unwrap().role, Role::TextRun);

    let pw = by_html_id(&t, "pw");
    assert_eq!(
        (pw.role, pw.name.as_str(), pw.value.as_deref()),
        (
            Role::PasswordInput,
            "Password",
            Some("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}")
        )
    );

    let remember = by_html_id(&t, "remember");
    assert_eq!(
        (
            remember.role,
            remember.name.as_str(),
            remember.states.checked
        ),
        (Role::CheckBox, "Remember me", Some(Checked::True))
    );
    let r1 = by_html_id(&t, "r1");
    assert_eq!(
        (r1.role, r1.name.as_str(), r1.states.checked),
        (Role::RadioButton, "Option one", Some(Checked::False))
    );

    let role = by_html_id(&t, "role");
    assert_eq!(
        (
            role.role,
            role.name.as_str(),
            role.value.as_deref(),
            role.states.expanded
        ),
        (Role::ComboBox, "Role", Some("Editor"), Some(false))
    );

    let vol = by_html_id(&t, "vol");
    assert_eq!((vol.role, vol.name.as_str()), (Role::Slider, "Volume"));
    assert_eq!(
        vol.numeric,
        Some(Numeric {
            now: Some(4.0),
            min: Some(0.0),
            max: Some(10.0),
            step: Some(1.0)
        })
    );
    assert!(vol.actions.contains(&A11yAction::Increment));

    let notes = by_html_id(&t, "notes");
    assert_eq!(
        (notes.role, notes.name.as_str()),
        (Role::MultilineTextInput, "Notes"),
        "placeholder fallback"
    );

    let ro = by_html_id(&t, "ro");
    assert!(ro.states.read_only && !ro.actions.contains(&A11yAction::SetValue));
    assert_eq!(ro.name, "Read only field");

    let bad = by_html_id(&t, "bad");
    assert_eq!(bad.name, "Broken field");
    assert!(bad.states.invalid);

    assert_eq!(by_html_id(&t, "save").name, "Save changes");
    let close = by_html_id(&t, "close");
    assert_eq!((close.role, close.name.as_str()), (Role::Button, "Close"));
    assert!(close.children.is_empty());
    assert_eq!(by_html_id(&t, "icon").name, "Delete");
    assert_eq!(by_html_id(&t, "go").name, "Submit");
    let dis = by_html_id(&t, "dis");
    assert!(dis.states.disabled);
    // Disabled controls cannot focus, edit or activate, but an assistive client may still
    // reveal them so the user can inspect their label and disabled state.
    assert_eq!(dis.actions, vec![A11yAction::ScrollIntoView]);
    let tog = by_html_id(&t, "tog");
    assert_eq!(
        (tog.states.checked, tog.states.expanded),
        (Some(Checked::True), Some(false))
    );

    let prog = by_html_id(&t, "prog");
    assert_eq!(
        (
            prog.role,
            prog.numeric.unwrap().now,
            prog.numeric.unwrap().max
        ),
        (Role::ProgressIndicator, Some(30.0), Some(100.0))
    );
    // Ordinary wrappers are flattened. A referenced labelling node is retained so the
    // native labelled-by relationship resolves to an exposed, stable target.
    assert!(
        t.nodes
            .values()
            .all(|n| n.role != Role::GenericContainer || n.html_id.as_deref() == Some("badlbl")),
        "{}",
        t.to_json()
    );
    assert_eq!(bad.relations.labelled_by, vec![by_html_id(&t, "badlbl").id]);
}

#[test]
fn tables_lists_and_landmarks() {
    let t = tree(
        r#"<body><header><h2>Brand</h2></header><aside>Side</aside>
        <table class="table" id="t"><caption>People</caption><thead><tr><th>Name</th><th scope="row">Age</th></tr></thead>
        <tbody><tr><td>Ann</td><td><a href="/a">Edit</a></td></tr></tbody></table>
        <ul id="l"><li>One</li><li>Two</li></ul><footer>f</footer></body>"#,
    );
    assert!(t.in_order().iter().any(|n| n.role == Role::Banner));
    assert!(t.in_order().iter().any(|n| n.role == Role::Complementary));
    assert!(t.in_order().iter().any(|n| n.role == Role::ContentInfo));
    let table = by_html_id(&t, "t");
    assert_eq!((table.role, table.name.as_str()), (Role::Table, "People"));
    let rows: Vec<&A11yNode> = table
        .children
        .iter()
        .filter_map(|c| t.get(*c))
        .filter(|n| n.role == Role::Row)
        .collect();
    assert_eq!(rows.len(), 2, "row groups flattened: {}", t.to_json());
    let header: Vec<(Role, &str)> = rows[0]
        .children
        .iter()
        .map(|c| t.get(*c).unwrap())
        .map(|n| (n.role, n.name.as_str()))
        .collect();
    assert_eq!(
        header,
        vec![(Role::ColumnHeader, "Name"), (Role::RowHeader, "Age")]
    );
    let cells: Vec<&A11yNode> = rows[1]
        .children
        .iter()
        .map(|c| t.get(*c).unwrap())
        .collect();
    assert_eq!((cells[0].role, cells[0].name.as_str()), (Role::Cell, "Ann"));
    assert_eq!(cells[1].name, "Edit");
    assert_eq!(
        t.get(cells[1].children[0]).unwrap().role,
        Role::Link,
        "structural children stay"
    );
    let list = by_html_id(&t, "l");
    assert_eq!(list.role, Role::List);
    assert_eq!(
        list.children
            .iter()
            .map(|c| t.get(*c).unwrap().role)
            .collect::<Vec<_>>(),
        vec![Role::ListItem, Role::ListItem]
    );
}

#[test]
fn dialogs_and_exclusion() {
    let t = tree(
        r#"<body>
        <div class="modal" role="dialog" aria-modal="true" aria-labelledby="mt" id="m"><h5 id="mt">Confirm delete</h5><button>OK</button></div>
        <dialog open id="d" aria-label="Native"><p>Hi</p></dialog>
        <div aria-hidden="true"><button id="h1">Hidden 1</button></div>
        <button hidden id="h2">Hidden 2</button>
        <div data-display-none><button id="h3">Hidden 3</button></div>
        <div data-visibility-hidden><button id="h4" data-visibility-hidden>Hidden 4</button><span>visible text</span></div>
        <img src="x.png" alt="Logo" id="img"><img src="y.png" alt="" id="deco">
        </body>"#,
    );
    let m = by_html_id(&t, "m");
    assert_eq!(
        (m.role, m.name.as_str(), m.states.modal),
        (Role::Dialog, "Confirm delete", true)
    );
    let d = by_html_id(&t, "d");
    assert_eq!((d.role, d.name.as_str()), (Role::Dialog, "Native"));
    for h in ["h1", "h2", "h3", "h4", "deco"] {
        assert!(
            t.in_order().iter().all(|n| n.html_id.as_deref() != Some(h)),
            "{h} must be excluded"
        );
    }
    assert!(
        t.in_order()
            .iter()
            .any(|n| n.role == Role::Label && n.name == "visible text"),
        "children of visibility:hidden may show"
    );
    let img = by_html_id(&t, "img");
    assert_eq!((img.role, img.name.as_str()), (Role::Image, "Logo"));
}

#[test]
fn diff_sends_changed_nodes_and_focus() {
    let a = tree_focused(
        r#"<body><button id="b">One</button><input id="i" value="x"></body>"#,
        None,
    );
    let b = tree_focused(
        r#"<body><button id="b">Two</button><input id="i" value="x"></body>"#,
        Some("i"),
    );
    assert!(diff_update(&a, &a).is_none());
    let u = diff_update(&a, &b).expect("changed");
    let changed: Vec<String> = u.nodes.iter().map(|(id, _)| node_label(id.0)).collect();
    // the button's name changed, the input gained focus (state)
    assert_eq!(u.nodes.len(), 2, "{changed:?}");
    assert_eq!(u.focus.0, by_html_id(&b, "i").id);
    let full = full_update(&b);
    assert_eq!(full.nodes.len(), b.nodes.len());
    assert_eq!(full.tree.as_ref().unwrap().root.0, ROOT_ID);
    let root = full.nodes.iter().find(|(id, _)| id.0 == ROOT_ID).unwrap();
    assert!(root.1.transform().is_some(), "device scale on the root");
}

#[test]
fn requests_decode_to_dom_nodes() {
    let t = tree(r#"<body><input id="i" value="x"></body>"#);
    let i = by_html_id(&t, "i");
    let run = i.children[0];
    let req = ActionRequest {
        action: Action::SetValue,
        target_tree: TreeId::ROOT,
        target_node: AkId(run),
        data: Some(ActionData::Value("hello".into())),
    };
    let r = decode_request(&req).unwrap();
    assert_eq!(
        r.target.map(node_key),
        Some(i.id),
        "text run resolves to its field"
    );
    assert_eq!(r.kind, A11yRequestKind::SetValue("hello".into()));
    let root = ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: AkId(ROOT_ID),
        data: None,
    };
    assert_eq!(decode_request(&root).unwrap().target, None);
    let custom = ActionRequest {
        action: Action::ShowTooltip,
        target_tree: TreeId::ROOT,
        target_node: AkId(i.id),
        data: None,
    };
    assert!(decode_request(&custom).is_none());
}

#[test]
fn expandable_buttons_offer_expand_collapse() {
    // UIA ExpandCollapse (beta Windows defect D1): a button with aria-expanded advertises Expand/Collapse and the
    // requests decode; a plain button does not.
    let t = tree(
        r#"<body><button id="m" aria-expanded="false" onclick="">Account</button><button id="p">Plain</button></body>"#,
    );
    let m = by_html_id(&t, "m");
    assert!(
        m.actions.contains(&A11yAction::Expand) && m.actions.contains(&A11yAction::Collapse),
        "{:?}",
        m.actions
    );
    assert!(!by_html_id(&t, "p").actions.contains(&A11yAction::Expand));
    for (a, k) in [
        (Action::Expand, A11yRequestKind::Expand),
        (Action::Collapse, A11yRequestKind::Collapse),
    ] {
        let req = ActionRequest {
            action: a,
            target_tree: TreeId::ROOT,
            target_node: AkId(m.id),
            data: None,
        };
        assert_eq!(decode_request(&req).unwrap().kind, k);
    }
}

#[test]
fn json_dump_is_tree_ordered() {
    let t = tree(FORM);
    let j = t.to_json();
    let nodes = j["nodes"].as_array().unwrap();
    assert_eq!(nodes[0]["role"], "Window");
    assert_eq!(nodes[1]["role"], "RootWebArea");
    assert!(
        nodes
            .iter()
            .any(|n| n["role"] == "Button" && n["name"] == "Close")
    );
}

#[test]
fn multiple_size_one_uses_combobox_role_and_multiple_rows_use_listbox() {
    let t = tree(
        r#"<select id=one multiple size="+1foo"><option selected>A</option><option selected>B</option></select>
                    <select id=two multiple size=2><option>A</option><option>B</option></select>"#,
    );
    let one = by_html_id(&t, "one");
    assert_eq!(one.role, Role::ComboBox);
    assert_eq!(one.value.as_deref(), Some("A"));
    assert_eq!(by_html_id(&t, "two").role, Role::ListBox);
    let mut doc = Document::parse_html(r#"<select id=s multiple size=1><option selected>A</option><option selected>B</option></select>"#).unwrap();
    let select = doc.element_by_id("s").unwrap();
    let role = |doc: &Document| {
        let tree = build(&BuildInput {
            document: doc,
            info: &Stub(doc),
            focused: None,
            title: "Win",
            viewport: (800.0, 600.0),
            scale: 1.0,
        });
        by_html_id(&tree, "s").role
    };
    assert_eq!(role(&doc), Role::ComboBox);
    doc.set_attribute(select, "size", "2").unwrap();
    assert_eq!(role(&doc), Role::ListBox);
    assert!(
        doc.select_options(select)
            .iter()
            .all(|o| doc.option_selected(*o))
    );
    doc.set_attribute(select, "size", "1").unwrap();
    assert_eq!(role(&doc), Role::ComboBox);
    assert!(
        doc.select_options(select)
            .iter()
            .all(|o| doc.option_selected(*o))
    );
}

#[test]
fn rendered_document_exposes_roles_and_names() {
    let mut d = crate::Document::parse(
        "<html><body><h1>Title</h1><button>Go</button><label for=n>Name</label><input id=n>\
         <input type=checkbox aria-label=Agree><select aria-label=Pick><option>A</option><option>B</option></select>\
         <a href='#x'>Link</a><p style='display:none'><button>Ghost</button></p></body></html>",
    )
    .unwrap();
    d.render(400, 300, 1.0).unwrap();
    let t = d.accessibility_tree().unwrap();
    t.validate().unwrap();
    let json = d.accessibility_json().unwrap().to_string();
    for (role, name) in [
        (Role::Heading, "Title"),
        (Role::Button, "Go"),
        (Role::TextInput, "Name"),
        (Role::CheckBox, "Agree"),
        (Role::ComboBox, "Pick"),
        (Role::Link, "Link"),
    ] {
        assert!(
            t.find(role, name).is_some(),
            "missing {role:?} {name:?} in {json}"
        );
    }
    assert!(
        !json.contains("Ghost"),
        "display:none content leaked: {json}"
    );
    let b = t.find(Role::Button, "Go").unwrap().bounds.unwrap();
    assert!(b.w > 0.0 && b.h > 0.0);
}

#[test]
fn requests_execute_on_a_rendered_document() {
    let mut d = crate::Document::parse(
        "<body><input type=checkbox aria-label=Agree><input aria-label=Who></body>",
    )
    .unwrap();
    d.render(400, 300, 1.0).unwrap();
    let t = d.accessibility_tree().unwrap();
    let cb = t.find(Role::CheckBox, "Agree").unwrap();
    let target = node_of_key(cb.id);
    d.accessibility_request(&A11yRequest {
        target,
        kind: A11yRequestKind::Click,
    })
    .unwrap();
    let t = d.accessibility_tree().unwrap();
    assert_eq!(
        t.find(Role::CheckBox, "Agree").unwrap().states.checked,
        Some(Checked::True)
    );
    let tx = node_of_key(t.find(Role::TextInput, "Who").unwrap().id);
    d.accessibility_request(&A11yRequest {
        target: tx,
        kind: A11yRequestKind::SetValue("Ada".into()),
    })
    .unwrap();
    let t = d.accessibility_tree().unwrap();
    assert_eq!(
        t.find(Role::TextInput, "Who").unwrap().value.as_deref(),
        Some("Ada")
    );
}

/// Live regions reach AccessKit: implicit (`role=status|log` polite, `alert` assertive) and
/// explicit `aria-live`, including an explicit `off` overriding the role. Without this, a status
/// line updated by the application is never announced by a screen reader.
#[test]
fn live_regions_map_implicit_and_explicit_politeness() {
    let t = tree(
        r#"<body><p id="s" role="status">Ready</p><div id="a" role="alert">Oops</div>
        <ul id="l" role="log"><li>one</li></ul><div id="p" aria-live="assertive">x</div>
        <p id="q" role="status" aria-live="off">quiet</p><p id="n">plain</p></body>"#,
    );
    let live = |id: &str| by_html_id(&t, id).states.live;
    assert_eq!(live("s"), super::LivePoliteness::Polite);
    assert_eq!(live("a"), super::LivePoliteness::Assertive);
    assert_eq!(live("l"), super::LivePoliteness::Polite);
    assert_eq!(live("p"), super::LivePoliteness::Assertive);
    assert_eq!(live("q"), super::LivePoliteness::Off);
    assert_eq!(live("n"), super::LivePoliteness::Off);
    let full = full_update(&t);
    let ak = |id: &str| {
        let want = by_html_id(&t, id).id;
        full.nodes
            .iter()
            .find(|(n, _)| n.0 == want)
            .unwrap()
            .1
            .live()
    };
    assert_eq!(ak("s"), Some(accesskit::Live::Polite));
    assert_eq!(ak("a"), Some(accesskit::Live::Assertive));
    assert_eq!(ak("n"), None);
}
