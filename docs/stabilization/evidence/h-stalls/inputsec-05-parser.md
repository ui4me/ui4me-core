# F-14 / INPUTSEC-05: quadratic parse of deep unclosed blocks (Linux, release)

`UI4ME_STALL_MEASURE=1 UI4ME_STALL_ONLY=unclosed cargo test --release -p ui4me-renderer --test
content_stalls -- --ignored --nocapture stall_measure` (`parse_hosted` of `<body>` + N x `<div>`).

| input | before (ms, result) | after (ms, result) |
|---|---|---|
| 5,000 unclosed `<div>` | 76, SIZE_LIMIT (depth, after the full parse) | 2.8, SIZE_LIMIT dom.parse_depth |
| 10,000 | 312 | 2.8 |
| 20,000 | 1,190 | 2.7 |
| 40,000 (audit divs40k, 200 KB) | 5,001 | 2.1 |
| 505 open `<div>` + 95,000 `<div></div>` (1 MiB, adversarial) | 412, SIZE_LIMIT dom.nodes (full parse) | 268, SIZE_LIMIT dom.nodes (stops at the node limit) |
| 250 open + 95,000 `<div></div>` | 208 | 140 |

Regression tests: `ui4me-document/tests/parser_corpus.rs` `a_parse_deeper_than_its_cap_stops_early`,
`a_capped_parse_stops_at_the_node_limit`, `chunked_parsing_builds_the_same_tree` (every html5lib
tree-construction case, document and fragment, parsed in chunks of 1, 2, 3, 7 and 61 bytes builds
the tree the whole-input parse builds); renderer `content_stalls.rs`
`deep_unclosed_blocks_are_refused_quickly` (load and `insert_html`; fails on the baseline: "load:
4.45s").

## Root cause and fix

html5ever scans the stack of open elements ("has a p element in button scope") for every block
start tag: O(depth) per tag, so nesting markup is quadratic, and the only bounds were the node limit
and the depth check after the full parse. html5ever cannot be stopped from the sink, so a parse with
a depth cap (`DomLimits::parse_depth`, new; unlimited by default; the hosted renderer sets
2 x MAX_DEPTH = 512) is fed in 4 KiB chunks and abandoned between chunks once the sink saw a node
deeper than the cap (`SIZE_LIMIT`, `dom.parse_depth`) or the arena over the node limit while no
`selectedcontent` element exists (`SIZE_LIMIT`, `dom.nodes`: the count only grows during a parse and
only `apply_selectedcontent` frees nodes afterwards, so the finished parse would fail the same way).
A fragment parse frees its staged nodes and leaves the document unchanged.

## Parse results

Documents within the hosted limits parse to the same tree: chunked feeding is html5ever's streaming
mode (corpus parity test above), and a document whose nodes all stay within depth 256 never comes
near the 512 cap. The cap could refuse a document only if html5ever's error recovery (the adoption
agency moving misnested content up) brought content deeper than 512 back within 256; that needs
more than 256 levels of misnested formatting elements.

Residual: markup that holds the open-element stack just under the cap and then emits tags until the
node limit still costs ~0.27 s (65,536 tags x ~500-deep scans).
