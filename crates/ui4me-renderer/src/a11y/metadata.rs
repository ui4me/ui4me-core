//! Structural metadata and relationships derived after the exposed tree is complete. Relations
//! are filtered against that tree; table inference respects row/column spans and nested tables.

use super::{A11yAction, A11yNode, Relations, SortOrder, node_key, node_of_key};
use accesskit::Role;
use std::collections::{BTreeMap, BTreeSet};
use ui4me_document::document::Document;

fn number(value: Option<&str>) -> Option<usize> {
    value
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&v| v > 0 && v <= u32::MAX as usize)
}
fn table(role: Role) -> bool {
    matches!(role, Role::Table | Role::Grid | Role::TreeGrid)
}
fn cell(role: Role) -> bool {
    matches!(
        role,
        Role::Cell | Role::GridCell | Role::ColumnHeader | Role::RowHeader
    )
}
fn item(role: Role) -> bool {
    matches!(
        role,
        Role::ListItem
            | Role::TreeItem
            | Role::ListBoxOption
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::RadioButton
            | Role::Tab
    )
}

pub(super) fn decorate(d: &Document, nodes: &mut BTreeMap<u64, A11yNode>) {
    let exposed: BTreeSet<_> = nodes.keys().copied().collect();
    let mut parents = BTreeMap::new();
    for (&id, node) in nodes.iter() {
        for &child in &node.children {
            parents.insert(child, id);
        }
    }
    let refs = |value: Option<&str>, self_id: u64| -> Vec<u64> {
        let mut found = Vec::new();
        for id in value.unwrap_or("").split_ascii_whitespace() {
            if let Some(n) = d.element_by_id(id) {
                let key = node_key(n);
                if key != self_id && exposed.contains(&key) && !found.contains(&key) {
                    found.push(key);
                }
            }
        }
        found
    };
    for (&id, node) in nodes.iter_mut() {
        if node.bounds.is_some()
            && !matches!(
                node.role,
                Role::Window | Role::RootWebArea | Role::ListBoxOption
            )
        {
            node.actions.push(A11yAction::ScrollIntoView);
        }
        // Synthetic text runs share a field's DOM identity but must not duplicate its metadata.
        let Some(dom) = node_of_key(id).filter(|&dom| node_key(dom) == id) else {
            continue;
        };
        let Some(e) = d.arena().get(dom).and_then(|n| n.as_element()) else {
            continue;
        };
        node.relations = Relations {
            labelled_by: refs(e.attr("aria-labelledby"), id),
            described_by: refs(e.attr("aria-describedby"), id),
            controls: refs(e.attr("aria-controls"), id),
            details: refs(e.attr("aria-details"), id),
            flow_to: refs(e.attr("aria-flowto"), id),
            owns: refs(e.attr("aria-owns"), id),
            active_descendant: refs(e.attr("aria-activedescendant"), id).first().copied(),
            error_message: refs(e.attr("aria-errormessage"), id).first().copied(),
        };
        let s = &mut node.structure;
        s.row_count = number(e.attr("aria-rowcount"));
        s.column_count = number(e.attr("aria-colcount"));
        s.row_index = number(e.attr("aria-rowindex"));
        s.column_index = number(e.attr("aria-colindex"));
        s.row_span = number(e.attr("aria-rowspan").or(e.attr("rowspan"))).map(|n| n.min(65534));
        s.column_span = number(e.attr("aria-colspan").or(e.attr("colspan"))).map(|n| n.min(1000));
        s.position_in_set = number(e.attr("aria-posinset"));
        if matches!(node.role, Role::ColumnHeader | Role::RowHeader) {
            s.sort_direction = match e
                .attr("aria-sort")
                .map(|v| v.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("ascending") => Some(SortOrder::Ascending),
                Some("descending") => Some(SortOrder::Descending),
                Some("other") => Some(SortOrder::Other),
                _ => None,
            };
        }
        if matches!(node.role, Role::TreeItem | Role::ListItem | Role::Row) {
            node.level = number(e.attr("aria-level"))
                .map(|n| n as u32)
                .or(node.level);
        }
        if e.local_name() == "select" && node.relations.active_descendant.is_none() {
            node.relations.active_descendant = d
                .select_options(dom)
                .into_iter()
                .find(|&n| d.option_selected(n))
                .map(node_key)
                .filter(|n| exposed.contains(n));
        }
    }

    let tables: Vec<_> = nodes
        .iter()
        .filter(|(_, n)| table(n.role))
        .map(|(&id, _)| id)
        .collect();
    for id in tables {
        decorate_table(d, nodes, id);
    }

    let containers: Vec<_> = nodes.keys().copied().collect();
    for container in containers {
        let members: Vec<_> = nodes[&container]
            .children
            .iter()
            .copied()
            .filter(|id| nodes.get(id).is_some_and(|n| item(n.role)))
            .collect();
        if members.is_empty() {
            continue;
        }
        let mut size = members.len();
        let mut known = true;
        for (i, &id) in members.iter().enumerate() {
            let dom = node_of_key(id);
            let explicit = dom.and_then(|dom| d.get_attribute(dom, "aria-setsize").ok().flatten());
            if explicit == Some("-1") {
                known = false;
            }
            if let Some(v) = number(explicit) {
                size = size.max(v);
            }
            let n = nodes.get_mut(&id).expect("member is exposed");
            let pos = n.structure.position_in_set.get_or_insert(i + 1);
            size = size.max(*pos);
            if n.role == Role::TreeItem && n.level.is_none() {
                let mut level = 1;
                let mut p = parents.get(&id).copied();
                while let Some(parent) = p {
                    if nodes.get(&parent).is_some_and(|n| n.role == Role::TreeItem) {
                        level += 1;
                    }
                    p = parents.get(&parent).copied();
                }
                nodes.get_mut(&id).expect("member is exposed").level = Some(level);
            }
        }
        if known {
            nodes
                .get_mut(&container)
                .expect("container exists")
                .structure
                .size_of_set = Some(size);
        }
    }
}

/// Descendants of one table excluding nested tables. Row groups may be flattened or semantic.
fn rows(nodes: &BTreeMap<u64, A11yNode>, table_id: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut stack: Vec<_> = nodes[&table_id].children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let n = &nodes[&id];
        if n.role == Role::Row {
            out.push(id);
        } else if !table(n.role) && !cell(n.role) {
            stack.extend(n.children.iter().rev().copied());
        }
    }
    out
}

fn decorate_table(d: &Document, nodes: &mut BTreeMap<u64, A11yNode>, id: u64) {
    let rows = rows(nodes, id);
    let mut occupied: Vec<(usize, usize, usize)> = Vec::new(); // first col, exclusive col, exclusive row
    let mut row_count = rows.len();
    let mut column_count = 0;
    for (offset, &row) in rows.iter().enumerate() {
        let row_index = nodes[&row].structure.row_index.unwrap_or(offset + 1);
        nodes.get_mut(&row).expect("row exists").structure.row_index = Some(row_index);
        row_count = row_count.max(row_index);
        occupied.retain(|&(_, _, until)| until > row_index);
        let children = nodes[&row].children.clone();
        let mut column = nodes[&row].structure.column_index.unwrap_or(1);
        for child in children {
            if !cell(nodes[&child].role) {
                continue;
            }
            let n = &nodes[&child];
            let col_span = n.structure.column_span.unwrap_or(1);
            let row_span = n.structure.row_span.unwrap_or(1);
            if let Some(explicit) = n.structure.column_index {
                column = explicit;
            }
            loop {
                let next = occupied
                    .iter()
                    .filter(|&&(a, b, _)| a < column.saturating_add(col_span) && b > column)
                    .map(|&(_, b, _)| b)
                    .max();
                match next {
                    Some(next) => column = next,
                    None => break,
                }
            }
            let end = column.saturating_add(col_span);
            let n = nodes.get_mut(&child).expect("cell exists");
            n.structure.row_index.get_or_insert(row_index);
            n.structure.column_index = Some(column);
            n.structure.row_span = Some(row_span);
            n.structure.column_span = Some(col_span);
            if row_span > 1 {
                occupied.push((column, end, row_index.saturating_add(row_span)));
            }
            column_count = column_count.max(end.saturating_sub(1));
            column = end;
        }
    }
    let dom = node_of_key(id);
    let explicit_rows = dom.and_then(|n| d.get_attribute(n, "aria-rowcount").ok().flatten());
    let explicit_columns = dom.and_then(|n| d.get_attribute(n, "aria-colcount").ok().flatten());
    let n = nodes.get_mut(&id).expect("table exists");
    if explicit_rows.is_none() {
        n.structure.row_count = Some(row_count);
    }
    if explicit_columns.is_none() {
        n.structure.column_count = Some(column_count);
    }
}
