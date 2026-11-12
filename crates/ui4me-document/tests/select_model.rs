//! owner: 012-interaction (lane L-CTRL-r10) — the `select` model (HTML 4.10.7 selectedness setting
//! algorithm, selectedIndex / value / option.selected, optgroup, disabled options).

use ui4me_document::document::Document;
use ui4me_engine_types::ids::NodeId;

fn id(d: &Document, i: &str) -> NodeId {
    d.element_by_id(i).unwrap()
}

fn parse(html: &str) -> Document {
    Document::parse_html(html).unwrap()
}

#[test]
fn a_dropdown_selects_its_first_enabled_option_by_default() {
    let d =
        parse("<select id=s><option disabled id=a>A<option id=b value=bv>B<option id=c>C</select>");
    let s = id(&d, "s");
    assert_eq!(d.select_selected_index(s), 1);
    assert_eq!(d.select_value(s), "bv");
    assert_eq!(d.select_display_label(s), "B");
    assert!(d.select_is_dropdown(s));
}

#[test]
fn the_last_selected_option_wins_in_a_single_select_and_multiple_keeps_all() {
    let d = parse(
        "<select id=s><option selected id=a>A<option selected id=b>B</select><select id=m multiple><option selected>A<option selected>B</select>",
    );
    assert_eq!(d.select_selected_index(id(&d, "s")), 1);
    let m = id(&d, "m");
    assert!(!d.select_is_dropdown(m));
    assert_eq!(d.select_display_size(m), 4);
    assert!(d.select_options(m).iter().all(|o| d.option_selected(*o)));
}

#[test]
fn optgroup_options_are_in_the_list_and_labels_and_values_follow_html() {
    let d = parse(
        "<select id=s size=3><optgroup label=G><option id=a label=Lbl value=v>  Text   here </option></optgroup><option id=b>  Two  words </option></select>",
    );
    let s = id(&d, "s");
    assert_eq!(d.select_options(s), vec![id(&d, "a"), id(&d, "b")]);
    assert_eq!(d.option_label(id(&d, "a")), "Lbl");
    assert_eq!(d.option_value(id(&d, "a")), "v");
    assert_eq!(d.option_text(id(&d, "a")), "Text here");
    assert_eq!(d.option_label(id(&d, "b")), "Two words");
    assert_eq!(d.option_value(id(&d, "b")), "Two words");
    // A listbox (display size 3) selects nothing by default.
    assert_eq!(d.select_selected_index(s), -1);
    assert!(!d.select_is_dropdown(s));
}

#[test]
fn host_writes_set_selected_index_value_and_option_selected() {
    let mut d = parse(
        "<select id=s><option id=a value=1>A<option id=b value=2>B<option id=c value=3>C</select>",
    );
    let s = id(&d, "s");
    d.select_set_selected_index(s, 2).unwrap();
    assert_eq!(
        (d.select_selected_index(s), d.select_value(s).as_str()),
        (2, "3")
    );
    d.select_set_value(s, "2").unwrap();
    assert_eq!(d.select_selected_index(s), 1);
    d.select_set_value(s, "nope").unwrap();
    assert_eq!(d.select_selected_index(s), -1);
    d.option_set_selected(id(&d, "a"), true).unwrap();
    assert_eq!(d.select_selected_index(s), 0);
    d.option_set_selected(id(&d, "c"), true).unwrap();
    assert_eq!(
        (d.select_selected_index(s), d.option_selected(id(&d, "a"))),
        (2, false)
    );
}

#[test]
fn dom_mutations_re_run_the_algorithm() {
    let mut d = parse("<select id=s><option id=a>A<option id=b>B</select>");
    let s = id(&d, "s");
    assert_eq!(d.select_selected_index(s), 0);
    d.set_attribute(id(&d, "a"), "disabled", "").unwrap();
    // The selected option stays selected when disabled (HTML only picks when none is selected).
    assert_eq!(d.select_selected_index(s), 0);
    let o = d.create_element("option").unwrap();
    d.set_attribute(o, "selected", "").unwrap();
    d.append_child(s, o).unwrap();
    assert_eq!(
        d.select_selected_index(s),
        2,
        "an inserted selected option wins"
    );
    d.remove_child(s, o).unwrap();
    // The selection is gone with the removed option; the first enabled option replaces it.
    assert_eq!(d.select_selected_index(s), 1);
    d.set_attribute(s, "multiple", "").unwrap();
    assert!(!d.select_is_dropdown(s));
}

#[test]
fn a_user_pick_selects_alone_or_toggles_in_a_multiple_select() {
    let mut d = parse(
        "<select id=s><option id=a>A<option id=b>B</select><select id=m multiple><option id=x>X<option id=y>Y</select>",
    );
    assert!(d.select_user_pick(id(&d, "b"), false).unwrap());
    assert_eq!(d.select_selected_index(id(&d, "s")), 1);
    assert!(
        !d.select_user_pick(id(&d, "b"), false).unwrap(),
        "no change"
    );
    assert!(d.select_user_pick(id(&d, "x"), true).unwrap());
    assert!(d.select_user_pick(id(&d, "y"), true).unwrap());
    assert!(d.option_selected(id(&d, "x")) && d.option_selected(id(&d, "y")));
    assert!(d.select_user_pick(id(&d, "x"), true).unwrap());
    assert!(!d.option_selected(id(&d, "x")));
}

#[test]
fn an_option_that_becomes_selected_wins_in_a_single_select() {
    let mut d = parse("<select id=s><option id=a>A<option id=b selected>B</select>");
    let s = id(&d, "s");
    assert_eq!(d.select_selected_index(s), 1);
    d.set_attribute(id(&d, "a"), "selected", "").unwrap();
    assert_eq!(
        d.select_selected_index(s),
        0,
        "the option whose selected attribute was just set wins"
    );
    assert!(!d.option_selected(id(&d, "b")));
}

#[test]
fn single_row_multiple_is_a_menulist_without_losing_selectedness() {
    let mut d = parse(
        "<select id=s multiple size='+1foo'><option id=a selected>A<option id=b selected>B<option id=c>C</select>",
    );
    let s = id(&d, "s");
    assert_eq!(d.select_display_size(s), 1);
    assert!(d.select_is_dropdown(s));
    assert!(d.option_selected(id(&d, "a")) && d.option_selected(id(&d, "b")));
    d.set_attribute(s, "size", "2trailing").unwrap();
    assert_eq!(d.select_display_size(s), 2);
    assert!(!d.select_is_dropdown(s));
    assert!(d.option_selected(id(&d, "a")) && d.option_selected(id(&d, "b")));
    d.set_attribute(s, "size", "01.5").unwrap();
    assert!(d.select_is_dropdown(s));
    assert!(d.option_selected(id(&d, "a")) && d.option_selected(id(&d, "b")));
    d.select_user_pick(id(&d, "c"), false).unwrap();
    assert!(
        !d.option_selected(id(&d, "a"))
            && !d.option_selected(id(&d, "b"))
            && d.option_selected(id(&d, "c"))
    );
}

#[test]
fn select_size_parser_matches_html_integer_prefix_and_defaults() {
    use ui4me_document::select::display_size;
    for (size, want) in [
        ("1foo", 1),
        (" +1foo", 1),
        ("01.5", 1),
        ("2x", 2),
        ("++1", 4),
        ("-1", 4),
        ("0", 4),
        ("4294967296", 4),
    ] {
        assert_eq!(display_size(Some(size), true), want, "{size:?}");
    }
    assert_eq!(display_size(None, true), 4);
    assert_eq!(display_size(None, false), 1);
}
