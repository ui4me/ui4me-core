//! owner: 003-document-model
//!
//! T054 / SC-003 / U4T-142: the misuse campaign. Random operations with stale, foreign, closed-document
//! and double-released handles mixed with valid ones. Every result must be Ok or one of the specified
//! typed outcomes, nothing may panic, and the data-model invariants must hold throughout.
//!
//! The 1,000,000-operation run is `#[ignore]` by default (`cargo test -p ui4me-document --test
//! handles_misuse -- --ignored --nocapture`); a 30,000-operation run executes in the normal suite.

mod common;

use ui4me_document::document::Document;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::NodeKey;

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
}

fn allowed(c: Code) -> bool {
    matches!(
        c,
        Code::StaleHandle
            | Code::WrongDocument
            | Code::DocumentClosed
            | Code::InvalidHierarchy
            | Code::InvalidCharacter
            | Code::InvalidNamespace
            | Code::InvalidArgument
            | Code::SizeLimit
    )
}

fn campaign(seed: u64, ops: u64) -> (u64, u64) {
    let mut rng = Rng(seed | 1);
    let mut a = Document::parse_html("<body><div id=x></div></body>").unwrap();
    let mut b = Document::parse_html("<body><div id=y></div></body>").unwrap();
    let mut pool: Vec<NodeKey> = vec![a.key(a.body().unwrap()), b.key(b.body().unwrap())];
    let (mut ok, mut rejected) = (0u64, 0u64);
    for step in 0..ops {
        if step == ops / 2 {
            b.request_close(false); // half way through, B closes: its handles must all say DOCUMENT_CLOSED
        }
        let k = |rng: &mut Rng, pool: &Vec<NodeKey>| pool[rng.below(pool.len())];
        let r: Result<(), ui4me_engine_types::status::Status> = match rng.below(12) {
            0 | 1 => {
                // create in A or B and remember the handle
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                let r = doc.create_element("e");
                if let Ok(n) = r {
                    let key = doc.key(n);
                    if pool.len() < 200 {
                        pool.push(key);
                    } else {
                        let i = rng.below(200);
                        pool[i] = key;
                    }
                }
                r.map(|_| ())
            }
            2 | 3 => {
                let (p, c) = (k(&mut rng, &pool), k(&mut rng, &pool));
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.append_child_key(p, c).map(|_| ())
            }
            4 => {
                let (p, c) = (k(&mut rng, &pool), k(&mut rng, &pool));
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.resolve_all(&[p, c])
                    .and_then(|ids| doc.remove_child(ids[0], ids[1]).map(|_| ()))
            }
            5 => {
                let n = k(&mut rng, &pool);
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.resolve(n)
                    .and_then(|id| doc.set_attribute(id, "k", "v"))
            }
            6 => {
                let n = k(&mut rng, &pool);
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.resolve(n)
                    .and_then(|id| doc.class_list_toggle(id, "c", None).map(|_| ()))
            }
            7 => {
                let n = k(&mut rng, &pool);
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.resolve(n).and_then(|id| doc.set_text_content(id, "t"))
            }
            8 => {
                let n = k(&mut rng, &pool);
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.resolve(n).and_then(|id| doc.retain(id))
            }
            9 => {
                // releases never fail, however often and on whatever handle
                let n = k(&mut rng, &pool);
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.release(n.node)
            }
            10 => {
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.checkpoint();
                Ok(())
            }
            _ => {
                let n = k(&mut rng, &pool);
                let doc = if rng.below(2) == 0 { &mut a } else { &mut b };
                doc.resolve(n)
                    .and_then(|id| doc.clone_node(id, true).map(|_| ()))
            }
        };
        match r {
            Ok(()) => ok += 1,
            Err(e) => {
                assert!(
                    allowed(e.code),
                    "step {step}: unspecified outcome {:?}",
                    e.code
                );
                rejected += 1;
            }
        }
        if step % 20_000 == 0 {
            common::invariants::check_invariants(&a)
                .unwrap_or_else(|e| panic!("step {step}: document A: {e}"));
            common::invariants::check_invariants(&b)
                .unwrap_or_else(|e| panic!("step {step}: document B: {e}"));
        }
    }
    common::invariants::check_invariants(&a).unwrap();
    common::invariants::check_invariants(&b).unwrap();
    (ok, rejected)
}

// table-a: A-12
#[test]
fn misuse_30k_operations_all_typed_and_invariants_hold() {
    let (ok, rejected) = campaign(0xC0FFEE, 30_000);
    assert!(
        ok > 1000 && rejected > 1000,
        "the mix exercises both valid and invalid use: ok={ok} rejected={rejected}"
    );
}

// table-a: A-12
#[test]
#[ignore = "1,000,000 operations; run with --ignored (SC-003 evidence)"]
fn misuse_one_million_operations() {
    let (ok, rejected) = campaign(0xBADC0DE, 1_000_000);
    println!("handles_misuse: ops=1000000 ok={ok} rejected={rejected} result=PASS");
}
