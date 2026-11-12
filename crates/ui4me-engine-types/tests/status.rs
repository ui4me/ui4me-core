// owner: 001-vertical-slice — T023 (core-types.md §4–§5; 004 error-mapping §2; AD-34)
use ui4me_engine_types::codes_generated::{Code, Family, SubReason};
use ui4me_engine_types::ids::DispatchIdSource;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

#[test]
fn status_carries_code_family_stage_subreason_and_diagnostic_id() {
    let mut ids = DispatchIdSource::new();
    let d = ids.next();
    let s = Status::new(Code::InvalidState, Stage::Host)
        .with_sub_reason(SubReason::RuntimeExists)
        .with_diagnostic_id(42)
        .with_message("second runtime")
        .with_dispatch_id(d)
        .with_batch_index(3)
        .with_byte_offset(17);
    assert_eq!(s.code, Code::InvalidState);
    assert_eq!(s.family(), Code::InvalidState.family());
    assert_eq!(s.stage, Stage::Host);
    assert_eq!(s.sub_reason, Some(SubReason::RuntimeExists));
    assert_eq!(s.diagnostic_id, 42);
    assert_eq!(s.message, "second runtime");
    assert_eq!(s.dispatch_id, Some(d));
    assert_eq!((s.batch_index, s.byte_offset), (Some(3), Some(17)));
    assert!(!s.is_ok());
    assert!(Status::ok().is_ok());
    assert_eq!(Status::ok().diagnostic_id, 0, "0 = none");
    let _: Family = s.family();
}

#[test]
fn stage_encoding_follows_004_error_mapping_section_2() {
    let expected = [
        (0u8, "none"),
        (1, "parse"),
        (2, "cascade"),
        (3, "box"),
        (4, "text"),
        (5, "layout"),
        (6, "scene"),
        (7, "raster"),
        (8, "event"),
        (9, "platform"),
        (10, "accessibility"),
        (11, "resource"),
        (12, "host"),
    ];
    assert_eq!(Stage::ALL.len(), expected.len());
    for (st, (code, name)) in Stage::ALL.iter().zip(expected) {
        assert_eq!(st.code(), code);
        assert_eq!(st.name(), name);
        assert_eq!(Stage::from_code(code), Some(*st));
    }
    assert_eq!(Stage::from_code(13), None);
}

#[test]
fn slice_codes_exist_with_004_values() {
    for (c, v) in [
        (Code::Ok, 0),
        (Code::StaleHandle, 101),
        (Code::WrongDocument, 102),
        (Code::DocumentClosed, 103),
        (Code::WrongThread, 201),
        (Code::InvalidArgument, 305),
        (Code::InvalidState, 306),
        (Code::SnapshotNotReady, 401),
        (Code::UnsupportedFeature, 501),
        (Code::GeometryNonFinite, 503),
    ] {
        assert_eq!(c.value(), v, "{}", c.name());
        assert_eq!(Code::from_value(v), Some(c));
    }
    for n in [
        "REENTRANT_CALL_FORBIDDEN",
        "CALLBACK_FAILED",
        "DISPATCH_RECURSION_LIMIT",
        "CANCELED",
        "FILE_NOT_FOUND",
        "RESOURCE_PENDING",
        "UNSUPPORTED_PAINT_FEATURE",
        "OUT_OF_MEMORY",
        "INTERNAL_INVARIANT",
    ] {
        assert!(
            Code::ALL.iter().any(|c| c.name() == n),
            "slice code {n} missing"
        );
    }
    assert!(SubReason::ALL.contains(&SubReason::NoWindows));
}

#[test]
fn no_alias_is_an_enum_member() {
    // Aliases are documentation-only (AD-34). An alias may be another spelling of its own
    // code (e.g. `StaleHandle` for STALE_HANDLE), but it must never name a *different* code,
    // and aliases never add enum members: the enum is exactly the registry's code list.
    let reg = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/errors.toml")).unwrap();
    let norm = |x: &str| {
        x.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_uppercase()
    };
    let mut current: Option<String> = None;
    let mut registry_codes = vec![];
    let mut checked = 0;
    for line in reg.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("code = ") {
            let c = v.trim_matches('"').to_owned();
            registry_codes.push(c.clone());
            current = Some(c);
        } else if let Some(rest) = l.strip_prefix("aliases") {
            let own = current.clone().expect("aliases belong to a code entry");
            let rest = rest.trim_start().trim_start_matches('=').trim();
            for a in rest
                .trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|a| a.trim().trim_matches('"'))
                .filter(|a| !a.is_empty())
            {
                checked += 1;
                for c in Code::ALL {
                    if c.name() != own {
                        assert_ne!(
                            norm(a),
                            norm(c.name()),
                            "alias {a} of {own} names another code {}",
                            c.name()
                        );
                    }
                }
            }
        }
    }
    assert!(checked > 0, "registry lists documentation-only aliases");
    let names: Vec<&str> = Code::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(
        names, registry_codes,
        "Code enum members are exactly the registry codes, in order"
    );
}
