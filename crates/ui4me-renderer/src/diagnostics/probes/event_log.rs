//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `event_log` probe: serialises `ui4me_engine_types::diag::Ring<DispatchRecord>` (schema §6); with
//! scope `document` only that document's dispatches. Producer arrives in US3.
//! Serialised by 001 T052.
//!
//! Shape decisions (T052): `pre_activation.saved` is the saved checkedness `{checked}`;
//! `status` is `ok` for `OK`, else the canonical code name; `event_data` key, code and data are
//! `{"redacted": true}` when the target is a password control (unless password values are
//! included); `nondeterministic.callback_token` is the token value.

use crate::diagnostics::envelope::{ExportCx, num};
use serde_json::{Map, Value, json};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{
    DispatchRecord, EventPhase, InvocationOutcome, InvocationRecord, Ring,
};

fn phase_name(p: EventPhase) -> &'static str {
    match p {
        EventPhase::Capturing => "capturing",
        EventPhase::AtTarget => "at-target",
        EventPhase::Bubbling => "bubbling",
    }
}

fn invocation_json(i: &InvocationRecord, cx: &ExportCx) -> Value {
    let mut listener = Map::new();
    listener.insert("ordinal".into(), json!(i.listener.ordinal));
    if let Some(l) = &i.listener.label {
        listener.insert("label".into(), json!(l));
    }
    listener.insert("capture".into(), json!(i.listener.capture));
    listener.insert("once".into(), json!(i.listener.once));
    listener.insert("passive".into(), json!(i.listener.passive));
    let mut m = Map::new();
    m.insert("seq".into(), json!(i.seq));
    m.insert("phase".into(), json!(phase_name(i.phase)));
    m.insert("current_target".into(), cx.key(i.current_target));
    m.insert("listener".into(), Value::Object(listener));
    m.insert(
        "outcome".into(),
        json!(match i.outcome {
            InvocationOutcome::Returned => "ok",
            InvocationOutcome::Failed => "callback-failed",
        }),
    );
    if let Some(d) = i.diagnostic_id.filter(|d| *d != 0) {
        m.insert("diagnostic_id".into(), json!(d));
    }
    m.insert(
        "calls".into(),
        Value::Array(i.calls.iter().map(|c| json!(c.name())).collect()),
    );
    m.insert(
        "default_prevented_after".into(),
        json!(i.default_prevented_after),
    );
    m.insert(
        "propagation_stopped_after".into(),
        json!(i.propagation_stopped_after),
    );
    m.insert(
        "immediate_stopped_after".into(),
        json!(i.immediate_stopped_after),
    );
    m.insert(
        "nested".into(),
        Value::Array(i.nested.iter().map(|d| json!(d.get().get())).collect()),
    );
    Value::Object(m)
}

fn dispatch_json(d: &DispatchRecord, cx: &ExportCx) -> Value {
    let mut m = Map::new();
    m.insert("document".into(), json!(cx.ordinal_of(d.document)));
    m.insert("dispatch_id".into(), json!(d.dispatch_id.get().get()));
    m.insert(
        "parent_dispatch_id".into(),
        d.parent_dispatch_id
            .map(|p| json!(p.get().get()))
            .unwrap_or(Value::Null),
    );
    m.insert("depth".into(), json!(d.depth));
    m.insert("type".into(), json!(d.event_type));
    m.insert("bubbles".into(), json!(d.bubbles));
    m.insert("cancelable".into(), json!(d.cancelable));
    m.insert("is_trusted".into(), json!(d.is_trusted));
    m.insert("origin".into(), json!(d.origin.name()));
    m.insert("target".into(), cx.key(d.target));
    m.insert(
        "path".into(),
        Value::Array(d.path.iter().map(|n| cx.key(*n)).collect()),
    );
    m.insert(
        "activation_target".into(),
        d.activation_target
            .map(|n| cx.key(n))
            .unwrap_or(Value::Null),
    );
    if let Some(p) = d.pre_activation {
        m.insert(
            "pre_activation".into(),
            json!({"performed": p.performed, "saved": {"checked": p.saved}}),
        );
    }
    m.insert(
        "invocations".into(),
        Value::Array(
            d.invocations
                .iter()
                .map(|i| invocation_json(i, cx))
                .collect(),
        ),
    );
    m.insert("canceled".into(), json!(d.canceled));
    if let Some(e) = &d.event_data {
        let redact =
            cx.password_targets.contains(&d.target) && !cx.redaction.password_values_included();
        let text = |v: &Option<String>| {
            v.as_ref().map(|s| {
                if redact {
                    json!({"redacted": true})
                } else {
                    json!(s)
                }
            })
        };
        let mut ed = Map::new();
        for (k, v) in [
            ("key", text(&e.key)),
            ("code", text(&e.code)),
            ("data", text(&e.data)),
        ] {
            if let Some(v) = v {
                ed.insert(k.into(), v);
            }
        }
        if let Some(b) = e.button {
            ed.insert("button".into(), json!(b));
        }
        if let Some(x) = e.client_x {
            ed.insert("client_x".into(), num(x));
        }
        if let Some(y) = e.client_y {
            ed.insert("client_y".into(), num(y));
        }
        if let Some(x) = e.delta_x {
            ed.insert("delta_x".into(), num(x));
        }
        if let Some(y) = e.delta_y {
            ed.insert("delta_y".into(), num(y));
        }
        if let Some(t) = &e.input_type {
            ed.insert("input_type".into(), json!(t));
        }
        if e.is_composing == Some(true) {
            ed.insert("is_composing".into(), json!(true));
        }
        if let Some(n) = &e.property_name {
            ed.insert("property_name".into(), json!(n));
        }
        if let Some(n) = &e.old_state {
            ed.insert("old_state".into(), json!(n));
        }
        if let Some(n) = &e.new_state {
            ed.insert("new_state".into(), json!(n));
        }
        if let Some(n) = &e.animation_name {
            ed.insert("animation_name".into(), json!(n));
        }
        if let Some(t) = e.elapsed_time {
            ed.insert("elapsed_time".into(), num(t));
        }
        if let Some(p) = &e.pseudo_element {
            ed.insert("pseudo_element".into(), json!(p));
        }
        m.insert("event_data".into(), Value::Object(ed));
    }
    m.insert(
        "status".into(),
        json!(if d.status == Code::Ok {
            "ok"
        } else {
            d.status.name()
        }),
    );
    let n = &d.nondeterministic;
    let mut nd = Map::new();
    if let Some(t) = n.time_ms {
        nd.insert("time_ms".into(), num(t));
    }
    if let Some(h) = &n.host_language {
        nd.insert("host_language".into(), json!(h));
    }
    if let Some(t) = n.callback_token {
        nd.insert("callback_token".into(), json!(t.get().get()));
    }
    m.insert("nondeterministic".into(), Value::Object(nd));
    Value::Object(m)
}

pub(crate) fn to_json(ring: &Ring<DispatchRecord>, cx: &ExportCx) -> Value {
    let dispatches: Vec<Value> = ring
        .iter()
        .filter(|d| cx.includes(d.document))
        .map(|d| dispatch_json(d, cx))
        .collect();
    // `capacity` and `dropped_count` are runtime-wide (schema §5).
    json!({"capacity": ring.capacity(), "dropped_count": ring.dropped(), "dispatches": dispatches})
}
