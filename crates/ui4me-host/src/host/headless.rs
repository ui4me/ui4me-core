//! F-12 (roadmap 2.1): the headless host runs the same frame pump as a native window, without a
//! surface, so frame-driven behaviour (transitions and animations, their events, the update that
//! consumes pending DOM writes) is observable in automated tests.
//!
//! * Every pump, a document with pending work gets a frame
//!   ([`ui4me_renderer::Document::frame_without_surface`]): style, layout and scene are
//!   committed as before a native present, and the events that produces are sent. Frames are
//!   paced like a 60 Hz display (at most one per [`FRAME`] of wall time per document), so a
//!   stream of DOM writes is coalesced the same way a native window coalesces it.
//! * The document clock stays real time, exactly as before, until the backend first calls the
//!   test-only request `test.advance_time {ms}`. From then on the host is on a deterministic
//!   virtual clock: every document (current and later loaded) uses a manual clock that moves only
//!   with `test.advance_time`, which runs the frames of that interval (16 ms steps) and delivers
//!   their events before it answers. A backend that never calls it sees no virtual time at all.
//! * `test.advance_time` exists only in headless test mode; a native host rejects it.
use super::*;

/// The display frame interval the headless pump emulates (and the virtual clock's step).
pub(super) const FRAME: Duration = Duration::from_millis(16);
/// The largest interval one `test.advance_time` may advance (bounds GUI-thread work per call).
pub(super) const MAX_ADVANCE_MS: f64 = 60_000.0;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvanceTime {
    ms: f64,
}

impl Host {
    /// Run the frame of every headless document that has pending work and whose last frame is at
    /// least one display frame ago (`force` ignores the pacing).
    pub(super) fn headless_frames(&mut self, force: bool) {
        if !self.headless {
            return;
        }
        let now = Instant::now();
        let due: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| !w.closing)
            .filter_map(|(id, w)| w.document.as_ref().map(|d| (id, d)))
            .filter(|(_, d)| d.engine.needs_repaint())
            .filter(|(_, d)| {
                force
                    || d.headless_frame_at
                        .is_none_or(|at| now.duration_since(at) >= FRAME)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.headless_frame(&id, now);
        }
    }

    fn headless_frame(&mut self, id: &str, now: Instant) {
        let Some((document_id, result)) = self
            .windows
            .get_mut(id)
            .and_then(|w| w.document.as_mut())
            .map(|d| {
                d.headless_frame_at = Some(now);
                (d.id.clone(), d.engine.frame_without_surface())
            })
        else {
            return;
        };
        if let Err(error) = result {
            self.document_error(id, &document_id, error);
        }
        // Drain and send the events this frame produced (transition and animation events
        // are dispatched when the host drains the document's queue).
        self.document_changed(id, None);
    }

    /// `test.advance_time {ms}`: move the virtual clock of every document by `ms` milliseconds
    /// in display-frame steps, running each frame and sending its events before the answer.
    pub(super) fn advance_time(&mut self, params: Value) -> Result<Value, RpcError> {
        if !self.headless {
            return Err(RpcError::unsupported(
                "test.advance_time is available only in headless test mode",
            ));
        }
        let request: AdvanceTime =
            serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?;
        if !request.ms.is_finite() || !(0.0..=MAX_ADVANCE_MS).contains(&request.ms) {
            return Err(RpcError::invalid_params(
                "ms must be a finite number of milliseconds from 0 to 60000",
            ));
        }
        if self.virtual_time_ms.is_none() {
            // Switch to the virtual clock: every document freezes its clock now; from here on
            // only this request moves time (documents loaded later start frozen too).
            self.virtual_time_ms = Some(0.0);
            for window in self.windows.values_mut() {
                if let Some(document) = &mut window.document {
                    document.engine.use_manual_clock();
                }
            }
        }
        // Frame 0: commit what is pending at the current virtual time, so a change made just
        // before this call starts its transitions now, as the next native frame would.
        let now = Instant::now();
        self.headless_frames(true);
        let step = FRAME.as_secs_f64() * 1000.0;
        let mut left = request.ms;
        let mut frames = 0u64;
        while left > 0.0 {
            let delta = left.min(step);
            left -= delta;
            frames += 1;
            let ids: Vec<_> = self
                .windows
                .iter()
                .filter(|(_, w)| !w.closing && w.document.is_some())
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                let Some((document_id, advanced)) = self
                    .windows
                    .get_mut(&id)
                    .and_then(|w| w.document.as_mut())
                    .map(|d| (d.id.clone(), d.engine.advance_clock(delta)))
                else {
                    continue;
                };
                if let Err(error) = advanced {
                    self.document_error(&id, &document_id, error);
                }
                self.headless_frame(&id, now);
            }
        }
        let total = self.virtual_time_ms.unwrap_or(0.0) + request.ms;
        self.virtual_time_ms = Some(total);
        Ok(json!({"advanced_ms":request.ms,"frames":frames,"virtual_time_ms":total}))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{add_window, controller};
    use super::*;

    const BUTTONS: &str = "<style>button{transition:background-color 120ms,color 120ms;\
        background:#eee;color:#111}.dark button{background:#333;color:#eee}</style>\
        <div id=root><button id=a>A</button><button id=b>B</button></div>";

    fn load(host: &mut Host, html: &str) -> String {
        host.dispatch_document("document.load", json!({"window_id":"w1","html":html}))
            .unwrap()["document_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn command(host: &mut Host, document: &str, op: &str, args: Value) -> Value {
        let mut command = json!({"op":op});
        command
            .as_object_mut()
            .unwrap()
            .extend(args.as_object().unwrap().clone());
        let params = json!({"window_id":"w1","document_id":document,"command":command});
        let result = host.dispatch_document("document.command", params).unwrap();
        host.document_changed("w1", None);
        result
    }

    fn event_types(host: &mut Host) -> Vec<String> {
        std::mem::take(&mut host.observed_notifications)
            .into_iter()
            .filter(|n| n["method"] == "document.event")
            .map(|n| n["params"]["event"]["type"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Without `test.advance_time` the host keeps the real-time clock (existing headless
    /// behaviour), but a style change is now committed by the frame pump without any read.
    #[test]
    fn frame_pump_commits_pending_changes_without_a_read() {
        let mut host = controller(true);
        add_window(&mut host, "w1");
        let document = load(&mut host, BUTTONS);
        host.headless_frames(true);
        event_types(&mut host);
        command(
            &mut host,
            &document,
            "add_class",
            json!({"element":"root","name":"dark"}),
        );
        assert!(
            host.windows["w1"]
                .document
                .as_ref()
                .unwrap()
                .engine
                .needs_repaint(),
            "the DOM write leaves work for the next frame"
        );
        assert!(event_types(&mut host).is_empty(), "nothing restyled yet");
        host.headless_frames(true);
        // The frame ran the style pass: the transitions exist and announce themselves.
        let events = event_types(&mut host);
        assert!(events.iter().any(|t| t == "transitionrun"), "{events:?}");
        assert!(
            host.virtual_time_ms.is_none(),
            "no virtual clock unless asked"
        );
    }

    /// T3 (unit level): the virtual clock delivers each control's transition events in order,
    /// all before the `advance_time` answer, and the same sequence on every run.
    #[test]
    fn advance_time_runs_transitions_deterministically_and_in_order() {
        let run = || {
            let mut host = controller(true);
            add_window(&mut host, "w1");
            let document = load(&mut host, BUTTONS);
            host.advance_time(json!({"ms":0})).unwrap();
            event_types(&mut host);
            command(
                &mut host,
                &document,
                "add_class",
                json!({"element":"root","name":"dark"}),
            );
            let answer = host.advance_time(json!({"ms":200})).unwrap();
            assert_eq!(answer["frames"], 13);
            assert_eq!(answer["virtual_time_ms"], 200.0);
            let events = event_types(&mut host);
            // A further advance after everything ended produces nothing.
            host.advance_time(json!({"ms":100})).unwrap();
            assert!(event_types(&mut host).is_empty());
            events
        };
        let first = run();
        let count = |kind: &str| first.iter().filter(|t| *t == kind).count();
        // 2 buttons x 2 transitioned properties.
        assert_eq!(count("transitionrun"), 4, "{first:?}");
        assert_eq!(count("transitionstart"), 4, "{first:?}");
        assert_eq!(count("transitionend"), 4, "{first:?}");
        let last_start = first.iter().rposition(|t| t == "transitionstart").unwrap();
        let first_end = first.iter().position(|t| t == "transitionend").unwrap();
        assert!(
            last_start < first_end,
            "run/start precede every end: {first:?}"
        );
        assert_eq!(first, run(), "the virtual clock is deterministic");
    }

    #[test]
    fn advance_time_validates_and_is_rejected_outside_test_mode() {
        let mut host = controller(true);
        for bad in [
            json!({}),
            json!({"ms":-1}),
            json!({"ms":60001}),
            json!({"ms":1,"x":2}),
        ] {
            assert_eq!(
                host.advance_time(bad.clone()).unwrap_err().code,
                protocol::code::INVALID_PARAMS,
                "{bad}"
            );
        }
        host.headless = false;
        assert_eq!(
            host.advance_time(json!({"ms":16})).unwrap_err().code,
            protocol::code::UNSUPPORTED
        );
    }

    /// RENDER-04: DOM writes without any read are consumed by the frame pump, so the journal
    /// cannot grow without bound in a headless soak.
    #[test]
    fn ten_thousand_writes_without_reads_are_committed_by_frames() {
        let mut host = controller(true);
        add_window(&mut host, "w1");
        let document = load(&mut host, "<p id=t>x</p>");
        for i in 0..10_000 {
            command(
                &mut host,
                &document,
                "set_text",
                json!({"element":"t","text":i.to_string()}),
            );
            if i % 64 == 63 {
                host.headless_frames(true);
                assert!(!host.windows["w1"]
                    .document
                    .as_ref()
                    .unwrap()
                    .engine
                    .needs_repaint());
            }
        }
    }
}
