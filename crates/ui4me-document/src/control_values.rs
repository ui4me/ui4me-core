//! owner: 012-interaction (lane L-CTRL-r10) on 003's stored control state — the value algorithms of
//! `input type=range|number`, `progress` and `meter` (WHATWG HTML sections 4.10.5.1.12 "Number",
//! 4.10.5.1.13 "Range", 4.10.5.15 "The step attribute", 4.10.13 "progress", 4.10.14 "meter").
//!
//! Pure reads over the attributes and the stored value; the typed host operations (`stepUp`,
//! `stepDown`, user steps of the keyboard and the pointer) are in [`Document::input_step`] and
//! write through [`ControlStateWriter`](crate::control_state::ControlStateWriter). Nothing here
//! fires an event.

use crate::document::Document;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// HTML "rules for parsing floating-point number values", restricted to what the controls use:
/// optional `-`, digits with an optional fraction, optional exponent; no leading `+`, no `inf`.
pub fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    let b = t.as_bytes();
    let mut i = 0;
    if b.first() == Some(&b'-') {
        i = 1;
    }
    let digits = |i: &mut usize| {
        let s0 = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - s0
    };
    let int = digits(&mut i);
    let mut frac = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        frac = digits(&mut i);
        if frac == 0 {
            return None;
        }
    }
    if int + frac == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Shortest decimal text of `v` (no exponent for ordinary magnitudes), after removing the binary
/// noise of repeated steps.
pub fn format_float(v: f64) -> String {
    let r = (v * 1e9).round() / 1e9;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

/// The numeric facts of a `range` or `number` input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepModel {
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// The allowed value step; `None` = `step=any`.
    pub step: Option<f64>,
    /// The step base: `min` when valid, else the `value` attribute (number), else 0.
    pub base: f64,
}

/// The region of a `meter` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterRegionKind {
    Optimum,
    Suboptimal,
    EvenLessGood,
}

impl Document {
    fn input_kind(&self, el: NodeId) -> Option<String> {
        let e = self
            .arena()
            .get(el)?
            .as_element()
            .filter(|e| e.is_html_named("input"))?;
        Some(e.attr("type").unwrap_or("text").trim().to_ascii_lowercase())
    }

    fn attr_f64(&self, el: NodeId, name: &str) -> Option<f64> {
        self.arena()
            .get(el)?
            .as_element()?
            .attr(name)
            .and_then(parse_float)
    }

    /// `min`, `max`, `step` and the step base of a `range` or `number` input (`None` for other
    /// types). A range's defaults are 0, 100 and 1, and its maximum is never below its minimum.
    pub fn input_step_model(&self, el: NodeId) -> Option<StepModel> {
        let kind = self.input_kind(el)?;
        let range = match kind.as_str() {
            "range" => true,
            "number" => false,
            _ => return None,
        };
        let mut min = self.attr_f64(el, "min").or(range.then_some(0.0));
        let mut max = self.attr_f64(el, "max").or(range.then_some(100.0));
        if range
            && let (Some(lo), Some(hi)) = (min, max)
            && hi < lo
        {
            max = Some(lo);
        }
        let step = match self
            .arena()
            .get(el)?
            .as_element()?
            .attr("step")
            .map(|s| s.trim().to_ascii_lowercase())
        {
            Some(s) if s == "any" => None,
            Some(s) => Some(parse_float(&s).filter(|v| *v > 0.0).unwrap_or(1.0)),
            None => Some(1.0),
        };
        let base = min
            .or_else(|| {
                if range {
                    None
                } else {
                    self.attr_f64(el, "value")
                }
            })
            .unwrap_or(0.0);
        if range && min.is_none() {
            min = Some(0.0);
        }
        Some(StepModel {
            min,
            max,
            step,
            base,
        })
    }

    /// The sanitised value of a `range` input (HTML value sanitization algorithm): the parsed value
    /// or the midpoint, clamped to the range and rounded to the nearest valid step.
    pub fn range_value(&self, el: NodeId) -> Option<f64> {
        let m = self.input_step_model(el)?;
        if self.input_kind(el)? != "range" {
            return None;
        }
        let (min, max) = (m.min.unwrap_or(0.0), m.max.unwrap_or(100.0));
        let raw = self.control_value(el).as_deref().and_then(parse_float);
        self.range_sanitize(
            el,
            raw.unwrap_or(if max < min {
                min
            } else {
                min + (max - min) / 2.0
            }),
        )
    }

    /// Clamp `v` to the range of a `range` input and round it to the nearest valid step.
    pub fn range_sanitize(&self, el: NodeId, v: f64) -> Option<f64> {
        let m = self.input_step_model(el)?;
        if self.input_kind(el)? != "range" {
            return None;
        }
        let (min, max) = (m.min.unwrap_or(0.0), m.max.unwrap_or(100.0));
        let mut v = v.clamp(min, max);
        if let Some(step) = m.step {
            let n = ((v - m.base) / step + 1e-9).round();
            let mut snapped = m.base + n * step;
            if snapped > max {
                snapped = m.base + (((max - m.base) / step + 1e-9).floor()) * step;
            }
            if snapped < min {
                snapped = m.base + (((min - m.base) / step - 1e-9).ceil()) * step;
            }
            v = snapped;
        }
        Some(v)
    }

    /// The position of a `range` value in `[min, max]`, 0..1.
    pub fn range_ratio(&self, el: NodeId) -> Option<f32> {
        let m = self.input_step_model(el)?;
        let v = self.range_value(el)?;
        let (min, max) = (m.min.unwrap_or(0.0), m.max.unwrap_or(100.0));
        Some(if max > min {
            ((v - min) / (max - min)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        })
    }

    /// `valueAsNumber` of a `range` or `number` input (`None` = NaN: empty or unparsable number).
    pub fn input_value_as_number(&self, el: NodeId) -> Option<f64> {
        match self.input_kind(el)?.as_str() {
            "range" => self.range_value(el),
            "number" => self.control_value(el).as_deref().and_then(parse_float),
            _ => None,
        }
    }

    /// `stepUp(n)` / `stepDown(-n)` (HTML 4.10.5.15) as a pure computation: the new value, or
    /// `None` when nothing changes. `n` is the signed number of steps.
    pub fn input_step_value(&self, el: NodeId, n: i32) -> Option<f64> {
        let m = self.input_step_model(el)?;
        let step = m.step.unwrap_or(1.0);
        let current = self.input_value_as_number(el);
        let mut value = current.unwrap_or(0.0);
        let before = value;
        let delta = f64::from(n) * step;
        let rem = (value - m.base) / step;
        if current.is_some() && (rem - rem.round()).abs() > 1e-9 && m.step.is_some() {
            // Not on a step: snap to the next valid value in the direction of the step.
            value = if n > 0 {
                m.base + (rem.ceil()) * step + (delta - step)
            } else {
                m.base + (rem.floor()) * step + (delta + step)
            };
        } else {
            value += delta;
        }
        if let Some(max) = m.max
            && value > max
        {
            value = if m.step.is_some() {
                m.base + (((max - m.base) / step + 1e-9).floor()) * step
            } else {
                max
            };
        }
        if let Some(min) = m.min
            && value < min
        {
            value = if m.step.is_some() {
                m.base + (((min - m.base) / step - 1e-9).ceil()) * step
            } else {
                min
            };
        }
        if (n < 0 && value > before)
            || (n > 0 && value < before)
            || (value - before).abs() < 1e-12 && current.is_some()
        {
            return None;
        }
        Some(value)
    }

    /// `progress`: the value as a fraction of `max`; `None` = indeterminate (no valid `value`).
    pub fn progress_ratio(&self, el: NodeId) -> Option<f32> {
        let e = self
            .arena()
            .get(el)?
            .as_element()
            .filter(|e| e.is_html_named("progress"))?;
        let max = e
            .attr("max")
            .and_then(parse_float)
            .filter(|m| *m > 0.0)
            .unwrap_or(1.0);
        let v = e
            .attr("value")
            .and_then(parse_float)
            .filter(|v| *v >= 0.0)?;
        Some((v.min(max) / max) as f32)
    }

    /// `meter`: the value as a fraction of `[min, max]` and its region.
    pub fn meter_state(&self, el: NodeId) -> Option<(f32, MeterRegionKind)> {
        let e = self
            .arena()
            .get(el)?
            .as_element()
            .filter(|e| e.is_html_named("meter"))?;
        let get = |n: &str| e.attr(n).and_then(parse_float);
        let min = get("min").unwrap_or(0.0);
        let max = get("max").unwrap_or(1.0).max(min);
        let value = get("value").unwrap_or(0.0).clamp(min, max);
        let low = get("low").unwrap_or(min).clamp(min, max);
        let high = get("high").unwrap_or(max).clamp(low, max);
        let optimum = get("optimum").unwrap_or((min + max) / 2.0).clamp(min, max);
        let region = if optimum < low {
            if value < low {
                MeterRegionKind::Optimum
            } else if value <= high {
                MeterRegionKind::Suboptimal
            } else {
                MeterRegionKind::EvenLessGood
            }
        } else if optimum > high {
            if value > high {
                MeterRegionKind::Optimum
            } else if value >= low {
                MeterRegionKind::Suboptimal
            } else {
                MeterRegionKind::EvenLessGood
            }
        } else if value >= low && value <= high {
            MeterRegionKind::Optimum
        } else {
            MeterRegionKind::Suboptimal
        };
        let ratio = if max > min {
            ((value - min) / (max - min)) as f32
        } else {
            0.0
        };
        Some((ratio, region))
    }

    /// The group of radio buttons of `radio` (HTML "radio button group"): same form owner (or both
    /// none, same tree), same non-empty `name`, in tree order; `radio` is in it.
    pub fn radio_group(&self, radio: NodeId) -> Vec<NodeId> {
        let Some(e) = self.arena().get(radio).and_then(|n| n.as_element()) else {
            return Vec::new();
        };
        let name = e.attr("name").unwrap_or("");
        if name.is_empty() || self.input_kind(radio).as_deref() != Some("radio") {
            return vec![radio];
        }
        let owner = e.form_owner;
        let root = self.arena().root_of(radio);
        self.arena()
            .subtree(root)
            .into_iter()
            .filter(|&n| {
                self.input_kind(n).as_deref() == Some("radio")
                    && self
                        .arena()
                        .get(n)
                        .and_then(|x| x.as_element())
                        .is_some_and(|x| x.attr("name") == Some(name) && x.form_owner == owner)
            })
            .collect()
    }

    /// Check `radio`: it becomes checked and every other radio of its group unchecked (HTML
    /// "checkedness setting algorithm for radio"). Dirty-making, like a user's check.
    pub fn radio_check(&mut self, radio: NodeId) -> Result<(), Status> {
        let group = self.radio_group(radio);
        self.check_writable(radio)?;
        self.begin_call();
        for r in group {
            if r != radio
                && self
                    .control_state(r)
                    .ok()
                    .flatten()
                    .and_then(|c| c.checkedness)
                    == Some(true)
            {
                let _ = self.control_state_writer().set_checkedness(r, false);
            }
        }
        let _ = self.control_state_writer().set_checkedness(radio, true);
        self.end_call();
        Ok(())
    }
}

impl Document {
    /// HTML "reset a form" (the controls' side; the `reset` event is 012's): every listed control owned
    /// by `form` returns to the default value, checkedness and selectedness its attributes give.
    pub fn form_reset_controls(&mut self, form: NodeId) -> Result<(), Status> {
        let controls = self.form_elements(form)?;
        self.check_writable(form)?;
        self.begin_call();
        for el in controls {
            let Some(e) = self.arena().get(el).and_then(|n| n.as_element()) else {
                continue;
            };
            match e.local_name() {
                "input" => {
                    let ty = e.attr("type").unwrap_or("text").trim().to_ascii_lowercase();
                    let checkable = ty == "checkbox" || ty == "radio";
                    let value = e
                        .attr("value")
                        .unwrap_or(if checkable { "on" } else { "" })
                        .to_owned();
                    let checked = checkable.then(|| e.attr("checked").is_some());
                    let _ = self.control_state_writer().reset_to_defaults(
                        el,
                        Some(value),
                        checked,
                        None,
                    );
                }
                "textarea" => {
                    let _ = self.control_state_writer().reset_to_defaults(
                        el,
                        Some(String::new()),
                        None,
                        None,
                    );
                }
                "select" => {
                    for o in self.select_options(el) {
                        let sel = self
                            .arena()
                            .get(o)
                            .and_then(|n| n.as_element())
                            .is_some_and(|x| x.attr("selected").is_some());
                        let _ =
                            self.control_state_writer()
                                .reset_to_defaults(o, None, None, Some(sel));
                    }
                    self.select_reset(el);
                }
                _ => {}
            }
        }
        self.end_call();
        Ok(())
    }
}
