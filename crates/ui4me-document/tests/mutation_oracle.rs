//! owner: 003-document-model
//!
//! T036 / SC-002 / U4T-142: the reference-model oracle. An independent, deliberately naive tree model
//! (vector children, explicit DOM algorithm steps written separately from the implementation) is run
//! in lockstep with `Document` over a generated sequence of structural, attribute, class, text,
//! cloning and handle-lifetime operations, including detached trees and stale handles. After every
//! step the full node graph, every result code and the data-model invariants must agree.
//!
//! `ORACLE_STEPS` (default 10,000) and `ORACLE_SEED` (default fixed) are echoed so the run can be
//! recorded as evidence.

mod common;

use std::collections::HashMap;
use ui4me_document::document::Document;
use ui4me_document::mutate::NodeOrString;
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Debug, PartialEq)]
enum K {
    Elem(String),
    Text,
    Comment,
    Frag,
}

#[derive(Clone, Debug)]
struct M {
    k: K,
    data: String,
    attrs: Vec<(String, String)>,
    parent: Option<usize>,
    kids: Vec<usize>,
    retained: u32,
    dead: bool,
}

type Outcome = Result<(), (Code, Option<SubReason>)>;

fn hr() -> Outcome {
    Err((Code::InvalidHierarchy, None))
}
fn not_found() -> Outcome {
    Err((Code::InvalidHierarchy, Some(SubReason::NotAChild)))
}
fn stale() -> Outcome {
    Err((Code::StaleHandle, None))
}
fn wrong_kind() -> Outcome {
    Err((Code::InvalidArgument, Some(SubReason::WrongNodeKind)))
}

struct Model {
    n: Vec<M>,
}

impl Model {
    fn new_node(&mut self, k: K, data: &str) -> usize {
        self.n.push(M {
            k,
            data: data.to_owned(),
            attrs: vec![],
            parent: None,
            kids: vec![],
            retained: 0,
            dead: false,
        });
        self.n.len() - 1
    }
    fn is_container(&self, i: usize) -> bool {
        matches!(self.n[i].k, K::Elem(_) | K::Frag)
    }
    fn inclusive_ancestor(&self, anc: usize, of: usize) -> bool {
        let mut cur = Some(of);
        while let Some(c) = cur {
            if c == anc {
                return true;
            }
            cur = self.n[c].parent;
        }
        false
    }
    fn detach(&mut self, i: usize) {
        if let Some(p) = self.n[i].parent.take() {
            self.n[p].kids.retain(|&c| c != i);
        }
    }
    fn first_dead(&self, ops: &[usize]) -> Option<usize> {
        ops.iter().copied().find(|&i| self.n[i].dead)
    }
    fn insert_at(&mut self, parent: usize, nodes: &[usize], reference: Option<usize>) {
        let at = reference
            .and_then(|r| self.n[parent].kids.iter().position(|&c| c == r))
            .unwrap_or(self.n[parent].kids.len());
        for (j, &x) in nodes.iter().enumerate() {
            self.n[parent].kids.insert(at + j, x);
            self.n[x].parent = Some(parent);
        }
    }
    fn next_sibling(&self, i: usize) -> Option<usize> {
        let p = self.n[i].parent?;
        let k = &self.n[p].kids;
        k.get(k.iter().position(|&c| c == i)? + 1).copied()
    }

    fn insert_before(&mut self, parent: usize, node: usize, child: Option<usize>) -> Outcome {
        let mut ops = vec![parent, node];
        ops.extend(child);
        if self.first_dead(&ops).is_some() {
            return stale();
        }
        if !self.is_container(parent) || self.inclusive_ancestor(node, parent) {
            return hr();
        }
        if let Some(c) = child
            && self.n[c].parent != Some(parent)
        {
            return not_found();
        }
        let reference = if child == Some(node) {
            self.next_sibling(node)
        } else {
            child
        };
        let nodes: Vec<usize> = if self.n[node].k == K::Frag {
            std::mem::take(&mut self.n[node].kids)
        } else {
            vec![node]
        };
        for &x in &nodes {
            self.n[x].parent = None;
        }
        if nodes.is_empty() {
            return Ok(());
        }
        if self.n[node].k != K::Frag {
            // node may still be listed under its old parent
            for p in 0..self.n.len() {
                if self.n[p].kids.contains(&node) {
                    self.n[p].kids.retain(|&c| c != node);
                }
            }
        }
        self.insert_at(parent, &nodes, reference);
        Ok(())
    }

    fn remove_child(&mut self, parent: usize, child: usize) -> Outcome {
        if self.first_dead(&[parent, child]).is_some() {
            return stale();
        }
        if !self.is_container(parent) {
            return hr();
        }
        if self.n[child].parent != Some(parent) {
            return not_found();
        }
        self.detach(child);
        Ok(())
    }

    fn remove(&mut self, node: usize) -> Outcome {
        if self.n[node].dead {
            return stale();
        }
        match self.n[node].parent {
            Some(p) => self.remove_child(p, node),
            None => Ok(()),
        }
    }

    fn replace(&mut self, parent: usize, node: usize, child: usize) -> Outcome {
        if self.first_dead(&[parent, node, child]).is_some() {
            return stale();
        }
        if !self.is_container(parent) || self.inclusive_ancestor(node, parent) {
            return hr();
        }
        if self.n[child].parent != Some(parent) {
            return not_found();
        }
        let flat: Vec<usize> = if self.n[node].k == K::Frag {
            self.n[node].kids.clone()
        } else {
            vec![node]
        };
        if flat == [child] {
            return Ok(());
        }
        let siblings = self.n[parent].kids.clone();
        let pos = siblings
            .iter()
            .position(|&c| c == child)
            .expect("child of parent");
        let reference = siblings[pos + 1..]
            .iter()
            .copied()
            .find(|c| !flat.contains(c));
        if self.n[node].k == K::Frag {
            for &x in &flat {
                self.n[x].parent = None;
            }
            self.n[node].kids.clear();
        } else if node != child {
            self.detach(node);
        }
        self.detach(child);
        self.insert_at(parent, &flat, reference);
        Ok(())
    }

    fn qualified(&self, i: usize, name: &str) -> String {
        let _ = i;
        name.to_ascii_lowercase()
    }

    fn valid_name(name: &str) -> bool {
        let mut c = name.chars();
        matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_' || f == ':')
            && c.all(|x| x.is_ascii_alphanumeric() || matches!(x, '_' | ':' | '-' | '.'))
    }

    fn set_attr(&mut self, el: usize, name: &str, val: &str) -> Outcome {
        if self.n[el].dead {
            return stale();
        }
        if !matches!(self.n[el].k, K::Elem(_)) {
            return wrong_kind();
        }
        if !Self::valid_name(name) {
            return Err((Code::InvalidCharacter, None));
        }
        let q = self.qualified(el, name);
        match self.n[el].attrs.iter_mut().find(|(k, _)| *k == q) {
            Some(a) => a.1 = val.to_owned(),
            None => self.n[el].attrs.push((q, val.to_owned())),
        }
        Ok(())
    }

    fn remove_attr(&mut self, el: usize, name: &str) -> Outcome {
        if self.n[el].dead {
            return stale();
        }
        if !matches!(self.n[el].k, K::Elem(_)) {
            return wrong_kind();
        }
        if !Self::valid_name(name) {
            return Err((Code::InvalidCharacter, None));
        }
        let q = self.qualified(el, name);
        self.n[el].attrs.retain(|(k, _)| *k != q);
        Ok(())
    }

    fn toggle_class(&mut self, el: usize, token: &str) -> Outcome {
        if self.n[el].dead {
            return stale();
        }
        if !matches!(self.n[el].k, K::Elem(_)) {
            return wrong_kind();
        }
        if token.is_empty() {
            return Err((Code::InvalidArgument, Some(SubReason::Syntax)));
        }
        if token.contains(' ') {
            return Err((Code::InvalidCharacter, None));
        }
        let cur = self.n[el]
            .attrs
            .iter()
            .find(|(k, _)| k == "class")
            .map(|(_, v)| v.clone());
        let mut set: Vec<String> = vec![];
        for t in cur.as_deref().unwrap_or("").split_ascii_whitespace() {
            if !set.iter().any(|x| x == t) {
                set.push(t.to_owned());
            }
        }
        if let Some(i) = set.iter().position(|x| x == token) {
            set.remove(i);
        } else {
            set.push(token.to_owned());
        }
        if cur.is_none() && set.is_empty() {
            return Ok(());
        }
        let joined = set.join(" ");
        match self.n[el].attrs.iter_mut().find(|(k, _)| k == "class") {
            Some(a) => a.1 = joined,
            None => self.n[el].attrs.push(("class".into(), joined)),
        }
        Ok(())
    }

    fn set_text(&mut self, node: usize, s: &str) -> Outcome {
        if self.n[node].dead {
            return stale();
        }
        match self.n[node].k {
            K::Text | K::Comment => self.n[node].data = s.to_owned(),
            K::Elem(_) | K::Frag => {
                for k in std::mem::take(&mut self.n[node].kids) {
                    self.n[k].parent = None;
                }
                if !s.is_empty() {
                    let t = self.new_node(K::Text, s);
                    self.n[node].kids.push(t);
                    self.n[t].parent = Some(node);
                }
            }
        }
        Ok(())
    }

    fn clone_deep(&mut self, src: usize, deep: bool) -> usize {
        let m = self.n[src].clone();
        let c = self.new_node(m.k.clone(), &m.data);
        self.n[c].attrs = m.attrs.clone();
        if deep {
            for k in m.kids {
                let kc = self.clone_deep(k, true);
                self.n[kc].parent = Some(c);
                self.n[c].kids.push(kc);
            }
        }
        c
    }

    fn checkpoint(&mut self) {
        loop {
            let mut changed = false;
            let roots: Vec<usize> = (0..self.n.len())
                .filter(|&i| i != 1 && !self.n[i].dead && self.n[i].parent.is_none())
                .collect();
            for r in roots {
                let mut sub = vec![r];
                let mut i = 0;
                while i < sub.len() {
                    sub.extend(self.n[sub[i]].kids.clone());
                    i += 1;
                }
                if sub.iter().all(|&x| self.n[x].retained == 0) {
                    for x in sub {
                        self.n[x].dead = true;
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

fn outcome(r: Result<(), ui4me_engine_types::status::Status>) -> Outcome {
    r.map_err(|e| (e.code, e.sub_reason))
}

struct Harness {
    d: Document,
    m: Model,
    ids: Vec<NodeId>,
    back: HashMap<NodeId, usize>,
}

impl Harness {
    fn new() -> Harness {
        let d = Document::parse_html("<!DOCTYPE html><body></body>").unwrap();
        let body = d.body().unwrap();
        // The model holds body (#0) plus its structural ancestors html (#1) and head (#2) so that a
        // body moved out of html becomes a collectible detached tree exactly as in the document.
        let mut m = Model { n: vec![] };
        let b = m.new_node(K::Elem("body".into()), "");
        let h = m.new_node(K::Elem("html".into()), "");
        let hd = m.new_node(K::Elem("head".into()), "");
        let html = d.document_element().unwrap();
        let head = d.head().unwrap();
        m.n[h].kids = vec![hd, b];
        m.n[b].parent = Some(h);
        m.n[hd].parent = Some(h);
        let mut back = HashMap::new();
        back.insert(body, b);
        back.insert(html, h);
        back.insert(head, hd);
        Harness {
            d,
            m,
            ids: vec![body, html, head],
            back,
        }
    }

    fn register(&mut self, id: NodeId) -> usize {
        // the model node was just pushed
        let i = self.m.n.len() - 1;
        self.ids.push(id);
        self.back.insert(id, i);
        i
    }

    /// Compare the whole live graph.
    fn compare(&self) -> Result<(), String> {
        for (i, mn) in self.m.n.iter().enumerate() {
            let id = self.ids[i];
            if mn.dead {
                if self.d.arena().contains(id) {
                    return Err(format!(
                        "model says #{i} is collected but the document still holds it"
                    ));
                }
                continue;
            }
            let node =
                self.d.arena().get(id).ok_or_else(|| {
                    format!("#{i} is live in the model but freed in the document")
                })?;
            let kids: Vec<usize> = node.children().iter().map(|c| self.back[c]).collect();
            if kids != mn.kids {
                return Err(format!("#{i}: children {kids:?} != model {:?}", mn.kids));
            }
            let parent = node.parent().and_then(|p| self.back.get(&p).copied());
            if parent != mn.parent {
                return Err(format!("#{i}: parent {parent:?} != model {:?}", mn.parent));
            }
            match (&mn.k, node.data()) {
                (K::Elem(t), ui4me_document::node::NodeData::Element(e)) => {
                    if e.local_name() != t {
                        return Err(format!("#{i}: tag {} != {t}", e.local_name()));
                    }
                    let attrs: Vec<(String, String)> = e
                        .attributes()
                        .iter()
                        .map(|a| (a.name.local.to_string(), a.value.clone()))
                        .collect();
                    if attrs != mn.attrs {
                        return Err(format!(
                            "#{i}: attributes {attrs:?} != model {:?}",
                            mn.attrs
                        ));
                    }
                }
                (K::Text, ui4me_document::node::NodeData::Text(t))
                | (K::Comment, ui4me_document::node::NodeData::Comment(t)) => {
                    if *t != mn.data {
                        return Err(format!("#{i}: data {t:?} != model {:?}", mn.data));
                    }
                }
                (K::Frag, ui4me_document::node::NodeData::DocumentFragment { .. }) => {}
                (k, d) => return Err(format!("#{i}: kind {k:?} vs {d:?}")),
            }
            if node.host_refs() != mn.retained {
                return Err(format!(
                    "#{i}: host refs {} != model {}",
                    node.host_refs(),
                    mn.retained
                ));
            }
        }
        common::invariants::check_invariants(&self.d)
    }

    fn step(&mut self, rng: &mut Rng) -> Result<String, String> {
        let live_cap = 70;
        let count = self.m.n.len();
        // html (#1) and head (#2) are structure only: operands are body or created nodes.
        let pick = |rng: &mut Rng| -> usize {
            let i = if rng.below(5) == 0 {
                0
            } else {
                rng.below(count)
            };
            if i == 1 || i == 2 { 0 } else { i }
        };
        let op = rng.below(100);
        let tags = ["div", "p", "span", "b"];
        let names = ["id", "class", "data-x", "Title", "a b", "1x", "", "x-Y"];
        let vals = ["", "v", "w z"];
        let tokens = ["a", "b", "", "c d", "a"];
        let texts = ["", "t", "hello"];
        let desc;
        let (exp, got): (Outcome, Outcome) = if count >= live_cap && op < 20 {
            desc = "checkpoint (cap)".to_owned();
            self.m.checkpoint();
            self.d.checkpoint();
            (Ok(()), Ok(()))
        } else if op < 5 {
            let t = *rng.pick(&tags);
            desc = format!("createElement {t}");
            let id = self.d.create_element(t).unwrap();
            self.m.new_node(K::Elem(t.into()), "");
            self.register(id);
            (Ok(()), Ok(()))
        } else if op < 8 {
            let s = *rng.pick(&texts);
            desc = format!("createTextNode {s:?}");
            let id = self.d.create_text_node(s).unwrap();
            self.m.new_node(K::Text, s);
            self.register(id);
            (Ok(()), Ok(()))
        } else if op < 9 {
            desc = "createComment".into();
            let id = self.d.create_comment("c").unwrap();
            self.m.new_node(K::Comment, "c");
            self.register(id);
            (Ok(()), Ok(()))
        } else if op < 11 {
            desc = "createDocumentFragment".into();
            let id = self.d.create_document_fragment().unwrap();
            self.m.new_node(K::Frag, "");
            self.register(id);
            (Ok(()), Ok(()))
        } else if op < 33 {
            let (p, n) = (pick(rng), pick(rng));
            desc = format!("appendChild #{p} #{n}");
            let exp = self.m.insert_before(p, n, None);
            (
                exp,
                outcome(self.d.append_child(self.ids[p], self.ids[n]).map(|_| ())),
            )
        } else if op < 45 {
            let (p, n, c) = (pick(rng), pick(rng), pick(rng));
            let c = if rng.below(4) == 0 { None } else { Some(c) };
            desc = format!("insertBefore #{p} #{n} {c:?}");
            let exp = self.m.insert_before(p, n, c);
            (
                exp,
                outcome(
                    self.d
                        .insert_before(self.ids[p], self.ids[n], c.map(|c| self.ids[c]))
                        .map(|_| ()),
                ),
            )
        } else if op < 52 {
            let (p, c) = (pick(rng), pick(rng));
            desc = format!("removeChild #{p} #{c}");
            let exp = self.m.remove_child(p, c);
            (
                exp,
                outcome(self.d.remove_child(self.ids[p], self.ids[c]).map(|_| ())),
            )
        } else if op < 56 {
            let n = pick(rng);
            desc = format!("remove #{n}");
            let exp = self.m.remove(n);
            (exp, outcome(self.d.remove(self.ids[n])))
        } else if op < 64 {
            let (p, n, c) = (pick(rng), pick(rng), pick(rng));
            desc = format!("replaceChild #{p} #{n} #{c}");
            let exp = self.m.replace(p, n, c);
            (
                exp,
                outcome(
                    self.d
                        .replace_child(self.ids[p], self.ids[n], self.ids[c])
                        .map(|_| ()),
                ),
            )
        } else if op < 72 {
            let (e, name, v) = (pick(rng), *rng.pick(&names), *rng.pick(&vals));
            desc = format!("setAttribute #{e} {name:?}={v:?}");
            let exp = self.m.set_attr(e, name, v);
            (exp, outcome(self.d.set_attribute(self.ids[e], name, v)))
        } else if op < 75 {
            let (e, name) = (pick(rng), *rng.pick(&names));
            desc = format!("removeAttribute #{e} {name:?}");
            let exp = self.m.remove_attr(e, name);
            (exp, outcome(self.d.remove_attribute(self.ids[e], name)))
        } else if op < 80 {
            let (e, t) = (pick(rng), *rng.pick(&tokens));
            desc = format!("classList.toggle #{e} {t:?}");
            let exp = self.m.toggle_class(e, t);
            (
                exp,
                outcome(self.d.class_list_toggle(self.ids[e], t, None).map(|_| ())),
            )
        } else if op < 85 {
            let (n, s) = (pick(rng), *rng.pick(&texts));
            desc = format!("textContent #{n} = {s:?}");
            let before = self.m.n.len();
            let exp = self.m.set_text(n, s);
            let got = outcome(self.d.set_text_content(self.ids[n], s));
            // the implementation allocates the new Text node; mirror it in the id table
            if exp.is_ok() && self.m.n.len() > before {
                let parent_id = self.ids[n];
                if let Some(&tid) = self.d.arena().node(parent_id).children().first() {
                    // align the model's new node index with the document's node id
                    let i = self.m.n.len() - 1;
                    self.ids.push(tid);
                    self.back.insert(tid, i);
                }
            }
            (exp, got)
        } else if op < 88 {
            let (n, deep) = (pick(rng), rng.below(2) == 0);
            desc = format!("cloneNode #{n} deep={deep}");
            if self.m.n[n].dead {
                (
                    stale(),
                    outcome(self.d.clone_node(self.ids[n], deep).map(|_| ())),
                )
            } else {
                let got = self.d.clone_node(self.ids[n], deep);
                match got {
                    Err(e) => (Ok(()), Err((e.code, e.sub_reason))),
                    Ok(cid) => {
                        let start = self.m.n.len();
                        let root = self.m.clone_deep(n, deep);
                        debug_assert_eq!(root, start);
                        // assign ids in parallel pre-order
                        let mut pairs = vec![(cid, root)];
                        let mut map: Vec<(usize, NodeId)> = vec![];
                        while let Some((di, mi)) = pairs.pop() {
                            map.push((mi, di));
                            let dk = self.d.arena().node(di).children().to_vec();
                            let mk = self.m.n[mi].kids.clone();
                            pairs.extend(dk.into_iter().zip(mk));
                        }
                        map.sort_by_key(|x| x.0);
                        for (mi, di) in map {
                            assert_eq!(self.ids.len(), mi, "model and id table stay aligned");
                            self.ids.push(di);
                            self.back.insert(di, mi);
                        }
                        (Ok(()), Ok(()))
                    }
                }
            }
        } else if op < 91 {
            let n = pick(rng);
            desc = format!("retain #{n}");
            let exp = if self.m.n[n].dead {
                stale()
            } else {
                self.m.n[n].retained += 1;
                Ok(())
            };
            (exp, outcome(self.d.retain(self.ids[n])))
        } else if op < 94 {
            let n = pick(rng);
            desc = format!("release #{n}");
            if !self.m.n[n].dead {
                self.m.n[n].retained = self.m.n[n].retained.saturating_sub(1);
            }
            (Ok(()), outcome(self.d.release(self.ids[n])))
        } else {
            desc = "checkpoint".into();
            self.m.checkpoint();
            self.d.checkpoint();
            (Ok(()), Ok(()))
        };
        if exp != got {
            return Err(format!(
                "{desc}: model expected {exp:?}, document returned {got:?}"
            ));
        }
        // a rejected or accepted call alike: the full graph must still agree
        self.compare().map_err(|e| format!("after {desc}: {e}"))?;
        Ok(desc)
    }
}

fn run(seed: u64, steps: usize) -> Result<(), String> {
    let mut h = Harness::new();
    let mut rng = Rng(seed | 1);
    let mut last = vec![];
    for i in 0..steps {
        match h.step(&mut rng) {
            Ok(d) => {
                last.push(d);
                if last.len() > 8 {
                    last.remove(0);
                }
            }
            Err(e) => return Err(format!("seed {seed} step {i}: {e}\nlast steps: {last:#?}")),
        }
    }
    Ok(())
}

// table-a: A-27, A-43, A-45
#[test]
fn the_document_agrees_with_the_reference_model_for_10000_steps() {
    let seed: u64 = std::env::var("ORACLE_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5eed_u64);
    let steps: usize = std::env::var("ORACLE_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);
    run(seed, steps).unwrap_or_else(|e| panic!("{e}"));
    println!("mutation_oracle: seed={seed} steps={steps} result=PASS");
}

// table-a: A-27
#[test]
fn many_short_runs_with_different_seeds() {
    for seed in 1..=40u64 {
        run(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), 400).unwrap_or_else(|e| panic!("{e}"));
    }
}

// table-a: A-32
#[test]
fn the_public_helpers_leave_the_invariants_intact() {
    // string operands and the ChildNode helpers are not in the model; they are exercised against the
    // invariant checker only.
    let mut d = Document::parse_html("<body><div id=a><p id=b></p></div></body>").unwrap();
    let a = d.element_by_id("a").unwrap();
    let b = d.element_by_id("b").unwrap();
    d.append(a, &[NodeOrString::Text("x".into())]).unwrap();
    d.before(b, &[NodeOrString::Text("y".into())]).unwrap();
    d.after(b, &[NodeOrString::Node(d.body().unwrap())])
        .unwrap_err();
    d.replace_children(a, &[NodeOrString::Text("z".into())])
        .unwrap();
    d.normalize(a).unwrap();
    common::invariants::check_invariants(&d).unwrap();
}
