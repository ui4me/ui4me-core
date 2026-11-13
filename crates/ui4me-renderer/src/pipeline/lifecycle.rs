//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Document lifecycle interplay (005 recompute-plan §3; lifecycle written by DocumentCore, R-27;
//! `Closing` suppression, R-34). Implemented by 001 T049.

use crate::pipeline::clock::AnimationClock;
use crate::pipeline::frame::FrameState;
use crate::pipeline::state::{DiagnosticStores, DocumentPipeline, OutputEpochs, PipelineConfig};
use crate::style::device::{ColorScheme, StyleEnvironment};
use crate::style::traverse::StyleBridge;
use crate::text::fonts::TextServices;
use crate::text::metrics::MetricsService;
use std::sync::Arc;
use ui4me_assets::fonts::{FontRegistry, FontSourcesView, PinnedFontConfig};
use ui4me_document::document::{Document, Lifecycle};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::DocumentId;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use ui4me_engine_types::text_metrics::{FontMetricsSource, FontSpec, GenericFamily, MetricsRecord};

/// 009's metrics service as the style engine's font-metrics source (AD-08).
#[derive(Debug)]
pub(crate) struct SharedMetrics(pub Arc<MetricsService>);

impl FontMetricsSource for SharedMetrics {
    fn query(&self, spec: &FontSpec, size: f32, vertical: bool) -> MetricsRecord {
        self.0.query(spec, size, vertical)
    }
    fn base_size_for_generic(&self, generic: Option<GenericFamily>) -> f32 {
        self.0.base_size_for_generic(generic)
    }
    fn font_epoch(&self) -> u64 {
        self.0.font_epoch()
    }
}

/// The pinned font sources of `config` (007 pinned-fonts-only mode).
pub fn pinned_font_sources(config: &PipelineConfig) -> Result<FontSourcesView, Status> {
    let reg = FontRegistry::pinned(&PinnedFontConfig::with_files(
        config.font_files.iter().cloned(),
    ))?;
    Ok(reg.font_sources())
}

/// A style bridge for `env` whose font metrics come from `text` (015 PAGE-1: the paged output's print stage).
pub(crate) fn style_bridge_for(
    document: DocumentId,
    env: StyleEnvironment,
    text: &TextServices,
) -> Result<StyleBridge, Status> {
    StyleBridge::with_font_metrics(
        document,
        env,
        Box::new(SharedMetrics(text.metrics_service())),
    )
}

/// A fresh style bridge and text services over `fonts` (live load and cold scratch pipeline).
pub(crate) fn fresh_stages(
    document: DocumentId,
    config: &PipelineConfig,
    fonts: &FontSourcesView,
    reduced_motion: bool,
    dark: bool,
) -> Result<(StyleBridge, TextServices), Status> {
    let text = TextServices::new(fonts)?;
    let mut env = StyleEnvironment::new(
        config.viewport_width as f32,
        config.viewport_height as f32,
        config.scale as f32,
    );
    env.reduced_motion = reduced_motion;
    env.color_scheme = if dark {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    };
    let style = StyleBridge::with_font_metrics(
        document,
        env,
        Box::new(SharedMetrics(text.metrics_service())),
    )?;
    Ok((style, text))
}

fn check_config(config: &PipelineConfig) -> Result<(), Status> {
    let ok = |v: f64| v.is_finite() && v >= 0.0;
    if !ok(config.viewport_width)
        || !ok(config.viewport_height)
        || !(config.scale.is_finite() && config.scale > 0.0)
    {
        return Err(
            Status::new(Code::InvalidArgument, Stage::Host).with_message(format!(
                "viewport {}x{} at scale {} is not a valid virtual viewport",
                config.viewport_width, config.viewport_height, config.scale
            )),
        );
    }
    Ok(())
}

/// The largest viewport (CSS px, 1/64 px grid of `LayoutUnit`) whose raster device size
/// (`ceil(css * f32(scale))`, 010) is exactly `device` on one axis. A window's CSS size is
/// `device / scale`, and the f32 scale widening or the layout grid can push the product just
/// over an integer; this keeps the frame the same size as the surface (011 presentation rule 2).
fn fit_axis(device: u32, scale: f64) -> f64 {
    let s = f64::from(scale as f32);
    let d = f64::from(device.max(1));
    let mut v = ((d / scale) * 64.0).floor() / 64.0;
    for _ in 0..256 {
        let dev = (v * s).ceil();
        if dev > d {
            v -= 1.0 / 64.0;
        } else if dev < d {
            v += 1.0 / 64.0;
        } else {
            break;
        }
    }
    v.max(0.0)
}

impl DocumentPipeline {
    /// Set the virtual viewport (CSS px) and device scale (005 environment input; P-C1). A change
    /// bumps the environment epoch and makes the document `Dirty`: the next update re-evaluates
    /// media queries and viewport units against a fresh environment and re-runs layout and paint
    /// (the committed set is replaced; the next frame is rastered at `viewport * scale`).
    /// Returns whether anything changed.
    pub fn set_environment(
        &mut self,
        viewport_width: f64,
        viewport_height: f64,
        scale: f64,
    ) -> Result<bool, Status> {
        self.check_open()?;
        let next = PipelineConfig {
            viewport_width,
            viewport_height,
            scale,
            ..self.config.clone()
        };
        check_config(&next)?;
        if next == self.config {
            return Ok(false);
        }
        // `ENVIRONMENT_VIEWPORT` when the CSS viewport size changed; `ENVIRONMENT_SCALE` for a
        // scale change alone (invalidation-reasons §1).
        let viewport_changed = next.viewport_width != self.config.viewport_width
            || next.viewport_height != self.config.viewport_height;
        self.config = next;
        self.frames.set_scale(scale);
        self.environment_epoch += 1;
        self.env_pending = true;
        let reason = if viewport_changed {
            crate::pipeline::reasons::Reason::EnvironmentViewport
        } else {
            crate::pipeline::reasons::Reason::EnvironmentScale
        };
        self.dirty
            .note(reason, crate::pipeline::reasons::Subject::Document);
        self.note_input();
        Ok(true)
    }

    /// [`DocumentPipeline::set_environment`] for a window whose client area is `device_width x
    /// device_height` device px at `scale`: the CSS viewport is chosen so that the rastered frame
    /// has exactly that device size (see `fit_axis`).
    pub fn set_device_viewport(
        &mut self,
        device_width: u32,
        device_height: u32,
        scale: f64,
    ) -> Result<bool, Status> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(Status::new(Code::InvalidArgument, Stage::Host)
                .with_message(format!("scale {scale} is not a valid device scale")));
        }
        self.set_environment(
            fit_axis(device_width, scale),
            fit_axis(device_height, scale),
            scale,
        )
    }

    /// Parse a full HTML document (AD-30) and instantiate its pipeline when the document becomes
    /// `Live`. Nothing is published yet: the state is `Dirty` until the first update.
    pub fn load_html(html: &str, config: PipelineConfig) -> Result<DocumentPipeline, Status> {
        check_config(&config)?;
        let fonts = pinned_font_sources(&config)?;
        let doc = Document::parse_html(html)?;
        DocumentPipeline::from_document(doc, config, fonts)
    }

    /// [`DocumentPipeline::load_html`] with a runtime-scoped document id (004) and font sources
    /// shared across the runtime's documents (007's registry is runtime-wide).
    pub fn load_html_with(
        document: DocumentId,
        html: &str,
        config: PipelineConfig,
        fonts: FontSourcesView,
    ) -> Result<DocumentPipeline, Status> {
        check_config(&config)?;
        let doc = Document::parse(document, html)?;
        DocumentPipeline::from_document(doc, config, fonts)
    }

    pub(crate) fn from_document(
        doc: Document,
        config: PipelineConfig,
        fonts: FontSourcesView,
    ) -> Result<DocumentPipeline, Status> {
        if doc.lifecycle() != Lifecycle::Live {
            return Err(Status::new(Code::InternalInvariant, Stage::Parse)
                .with_message("parsed document is not Live"));
        }
        let (style, text) = fresh_stages(doc.id(), &config, &fonts, false, false)?;
        let scale = config.scale;
        Ok(DocumentPipeline {
            doc,
            config,
            fonts,
            exported_faces: Vec::new(),
            font_rebuild_pending: false,
            text,
            style,
            // The parsed DOM is an input the committed set (none yet) does not reflect.
            machine: crate::pipeline::state::StateMachine::new(),
            facts: None,
            dirty: crate::pipeline::dirty::DirtySets::default(),
            views: crate::pipeline::views::Views::default(),
            perf: ui4me_engine_types::diag::PerformanceStore::default(),
            last_stage_ms: ui4me_engine_types::diag::StageMs::default(),
            earliest_mutation: None,
            no_animation_paint_path: false,
            no_layout_reuse: false,
            scene_reuse: std::env::var_os("UI4ME_SCENE_REUSE").is_none_or(|v| v != "0"), // F1004: on (qualified by the F1006 soak and the long-document RSS tests); UI4ME_SCENE_REUSE=0 turns it off.
            no_damage_raster: false,
            font_gen: 0,
            layout_font_gen: 0,
            box_styles: Default::default(),
            file_input_labels: Default::default(),
            file_input_labels_dirty: false,
            paint_styles: Default::default(),
            layout_reuses: 0,
            animation_paint_updates: 0,
            environment_epoch: 1,
            resource_epoch: 0,
            scroll_epoch: 0,
            scroll: Default::default(),
            caret_visible: true,
            caret_upstream: false,
            env_pending: false,
            font_context_pending: false,
            reduced_motion: false,
            dark: false,
            committed: None,
            epochs: OutputEpochs::default(),
            generation: 0,
            updates_run: 0,
            fault: None,
            #[cfg(feature = "fault-injection")]
            faults: crate::pipeline::fault::FaultPlan::default(),
            stores: DiagnosticStores::default(),
            frames: FrameState::new(scale),
            clock: AnimationClock::manual(),
            images: None,
            resources: None,
        })
    }

    /// The lifecycle written by DocumentCore (R-27).
    pub fn lifecycle(&self) -> Lifecycle {
        self.doc.lifecycle()
    }

    /// `Closing` or `Closed`: DOM calls and derived reads give `DOCUMENT_CLOSED`, no trigger is
    /// honoured and no frame is presented (R-34).
    pub fn is_closing(&self) -> bool {
        matches!(self.doc.lifecycle(), Lifecycle::Closing | Lifecycle::Closed)
    }

    /// A close request: `Closing` inside a dispatch, `Closed` immediately outside (AD-30).
    pub fn request_close(&mut self, dispatch_active: bool) {
        self.doc.request_close(dispatch_active);
    }

    /// COMMIT of the outermost dispatch (applies a deferred close).
    pub fn outermost_commit(&mut self) {
        self.doc.outermost_commit();
    }

    /// O1 for a closing document.
    pub(crate) fn check_open(&self) -> Result<(), Status> {
        if self.is_closing() {
            return Err(Status::new(Code::DocumentClosed, Stage::Host)
                .with_message(format!("document is {}", self.doc.lifecycle().name())));
        }
        Ok(())
    }
}

/// The family name each registered blob contributes (009's collection), registration order.
pub fn blob_families(fonts: &FontSourcesView) -> Result<Vec<String>, Status> {
    fonts
        .registered
        .iter()
        .map(|blob| {
            // only the blob's own family is wanted: never load the platform collection for it
            let single = FontSourcesView {
                registered: vec![blob.clone()],
                system_fonts: false,
                faces: Vec::new(),
                ..fonts.clone()
            };
            let env = crate::text::fonts::FontEnvironment::new(&single)?;
            Ok(env
                .family_names()
                .first()
                .cloned()
                .unwrap_or_else(|| blob.name.clone()))
        })
        .collect()
}
