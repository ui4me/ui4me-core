//! CSS `resize` (CSS UI 4; 0.9 F0903): a primary press within the 14 CSS px bottom-right corner
//! of an element with `resize` other than `none` and `overflow` other than `visible` starts a
//! resize; pointer moves set the element's inline `width`/`height` (the axes `resize` allows,
//! never below 1 px or the border and padding), as browsers do; the release ends it. The press
//! and moves of a resize do not reach the page.

use super::*;

const GRIP: f64 = 14.0;

/// (element, horizontal, vertical, pointer start, content-box size at the start).
type ActiveResize = (NodeId, bool, bool, (f64, f64), (f64, f64));

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ResizeState {
    /// The element, its axes, the pointer start and the content-box size at the start.
    active: Option<ActiveResize>,
}

fn px(v: Option<String>) -> f64 {
    v.and_then(|s| s.trim().strip_suffix("px").and_then(|n| n.parse().ok()))
        .unwrap_or(0.0)
}

impl Document {
    fn resize_style(&self, n: NodeId) -> Option<(bool, bool, (f64, f64))> {
        let v = self.pipeline.committed.as_deref()?.style.view(n)?;
        let (h, vert) = match v.serialize("resize")?.as_str() {
            "both" => (true, true),
            "horizontal" | "inline" => (true, false),
            "vertical" | "block" => (false, true),
            _ => return None,
        };
        if v.serialize("overflow-x").as_deref() == Some("visible")
            && v.serialize("overflow-y").as_deref() == Some("visible")
        {
            return None;
        }
        let sides = |a: &str, b: &str| {
            px(v.serialize(&format!("padding-{a}")))
                + px(v.serialize(&format!("padding-{b}")))
                + px(v.serialize(&format!("border-{a}-width")))
                + px(v.serialize(&format!("border-{b}-width")))
        };
        let border_box = v.serialize("box-sizing").as_deref() == Some("border-box");
        let extra = if border_box {
            (0.0, 0.0)
        } else {
            (sides("left", "right"), sides("top", "bottom"))
        };
        Some((h, vert, extra))
    }
    /// Handle the input when it starts, continues or ends a resize; true = consumed.
    pub(super) fn resize_input(&mut self, input: &Input) -> Result<bool> {
        match *input {
            Input::PointerButton {
                pressed: true,
                button: 0,
                x,
                y,
            } => {
                let id = self.id();
                let Some(mut n) =
                    crate::interaction::dispatch::DispatchHost::hit_test(self, id, x, y)
                        .ok()
                        .flatten()
                else {
                    return Ok(false);
                };
                // The resizable element is the hit element or an ancestor whose corner is hit.
                loop {
                    if let (Some((h, v, extra)), Some((rx, ry, rw, rh))) =
                        (self.resize_style(n), self.viewport_rect(n))
                        && x >= rx + rw - GRIP
                        && x <= rx + rw
                        && y >= ry + rh - GRIP
                        && y <= ry + rh
                    {
                        let size = (rw - extra.0, rh - extra.1);
                        self.resize.active = Some((n, h, v, (x, y), size));
                        return Ok(true);
                    }
                    match self
                        .pipeline
                        .document()
                        .arena()
                        .get(n)
                        .and_then(|x| x.parent())
                    {
                        Some(p)
                            if self
                                .pipeline
                                .document()
                                .arena()
                                .get(p)
                                .is_some_and(|x| x.as_element().is_some()) =>
                        {
                            n = p
                        }
                        _ => return Ok(false),
                    }
                }
            }
            Input::PointerMoved { x, y } => {
                let Some((n, h, v, start, size)) = self.resize.active else {
                    return Ok(false);
                };
                if !self.pipeline.document().arena().is_connected(n) {
                    self.resize.active = None;
                    return Ok(false);
                }
                let current = self
                    .pipeline
                    .document()
                    .get_attribute(n, "style")
                    .ok()
                    .flatten()
                    .unwrap_or("")
                    .to_owned();
                let mut decls: Vec<String> = current
                    .split(';')
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .filter(|d| {
                        let name = d
                            .split(':')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_ascii_lowercase();
                        !(h && name == "width" || v && name == "height")
                    })
                    .map(str::to_owned)
                    .collect();
                if h {
                    decls.push(format!(
                        "width: {}px",
                        (size.0 + x - start.0).max(1.0).round()
                    ));
                }
                if v {
                    decls.push(format!(
                        "height: {}px",
                        (size.1 + y - start.1).max(1.0).round()
                    ));
                }
                let style = decls.join("; ");
                let hosted = self.hosted;
                let r: Result<()> = self
                    .pipeline
                    .mutate(|d| Ok(model_set_attribute(d, n, "style", Some(&style), hosted)))?;
                r?;
                self.after_mutation(false);
                Ok(true)
            }
            Input::PointerButton {
                pressed: false,
                button: 0,
                ..
            } if self.resize.active.is_some() => {
                self.resize.active = None;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
