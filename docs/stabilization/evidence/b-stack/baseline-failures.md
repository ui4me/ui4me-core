# Tests that fail on the baseline

F-06 tests (crates/ui4me-renderer/tests/stack_depth.rs) with the renderer document.rs/box tree and the
assets sources stashed back to the baseline (`cargo test --release -p ui4me-renderer --test stack_depth`):

```
test stack_measure ... ignored, measurement tool; run explicitly (see the module documentation)
test stack_probe ... ignored, child-process entry of the stack-depth tests
test insert_html_counts_insertion_depth ... FAILED
test a_rejected_move_rolls_back_its_batch ... FAILED
test move_chains_cannot_exceed_the_limit ... FAILED
test text_below_the_deepest_element_is_refused ... FAILED
test replace_children_html_counts_from_the_element ... FAILED
test move_element_rejects_depth_over_limit ... FAILED
test one_level_beyond_the_maximum_is_rejected_not_rendered ... ok
test documented_maximum_depth_survives_half_the_engine_stack ... ok
thread '<unnamed>' (459594) panicked at crates/ui4me-renderer/tests/stack_depth.rs:265:15:
expected SIZE_LIMIT: ()
thread '<unnamed>' (459596) panicked at crates/ui4me-renderer/tests/stack_depth.rs:400:33:
thread '<unnamed>' (459592) panicked at crates/ui4me-renderer/tests/stack_depth.rs:265:15:
expected SIZE_LIMIT: ()
thread '<unnamed>' (459599) panicked at crates/ui4me-renderer/tests/stack_depth.rs:265:15:
expected SIZE_LIMIT: ()
thread '<unnamed>' (459598) panicked at crates/ui4me-renderer/tests/stack_depth.rs:361:49:
called `Result::unwrap()` on an `Err` value: Error { code: 5, message: "SIZE_LIMIT: Document node/depth limit exceeded" }
thread '<unnamed>' (459593) panicked at crates/ui4me-renderer/tests/stack_depth.rs:265:15:
expected SIZE_LIMIT: ()
test result: FAILED. 2 passed; 6 failed; 2 ignored; 0 measured; 0 filtered out; finished in 5.75s
```

documented_maximum_depth_survives_half_the_engine_stack passes on the baseline *renderer* because the
test uses the new 16 MiB half-engine stack: the 1.0.1 defect was the stack the host gave the renderer.
At half the stacks the 1.0.1 host actually had, the baseline renderer aborts (stack-per-level.md):
div/fieldset/flex at 4 MiB (half the Linux main thread) and every kind at 512 KiB (half the Windows reserve).
The fixed renderer needs at most 2.9 MiB.

SVG (crates/ui4me-assets/tests/svg_depth.rs) uses the new pre-scan API and does not compile against the
baseline; the baseline behaviour is the host abort in host-level.md (svgdepth.py 700, clipdepth.py 2000)
(both abort the 1.0.1 host on its 2 MiB default worker stack); on the fixed tree the pre-scan refuses both
before usvg runs (beyond_the_cap_is_a_clean_decode_failure).
