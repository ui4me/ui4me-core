//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Per-document style `Device` (T042; AD-08; 006 plan §8): `MediaData` from the environment
//! published by HostRuntime/WindowPresenter (viewport, device size, DPR, colour scheme) and the
//! font-metrics adapter (font_metrics.rs).

use crate::style::font_metrics::MetricsAdapter;
use style::context::QuirksMode;
use style::device::Device;
use style::device::servo::MediaData;
use style::media_queries::MediaType;
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::{PrefersColorScheme, PrefersReducedMotion};
use ui4me_engine_types::text_metrics::FontMetricsSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    Light,
    Dark,
}

/// The media environment of one document (CSS px; `device_pixel_ratio` = scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleEnvironment {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub device_pixel_ratio: f32,
    pub color_scheme: ColorScheme,
    /// `prefers-reduced-motion: reduce` (P-006-1); the OS/host/test-mode value resolved by the runtime.
    pub reduced_motion: bool,
    /// The `print` media type (015 PAGE-1: paginated output; `screen` otherwise).
    pub print: bool,
}

impl StyleEnvironment {
    /// The windowless default viewport: 800 x 600 CSS px at scale 1.0 (OD-42, pending owner
    /// decision; the host can set another size).
    pub fn windowless_default() -> StyleEnvironment {
        StyleEnvironment::new(800.0, 600.0, 1.0)
    }

    pub fn new(
        viewport_width: f32,
        viewport_height: f32,
        device_pixel_ratio: f32,
    ) -> StyleEnvironment {
        StyleEnvironment {
            viewport_width,
            viewport_height,
            device_pixel_ratio,
            color_scheme: ColorScheme::Light,
            reduced_motion: false,
            print: false,
        }
    }
}

pub(crate) fn build_device(
    env: &StyleEnvironment,
    quirks: QuirksMode,
    metrics: Box<dyn FontMetricsSource>,
) -> Device {
    Device::new(
        ComputedValues::initial_values_with_font_override(Font::initial_values()),
        Box::new(MetricsAdapter(metrics)),
        MediaData {
            media_type: if env.print {
                MediaType::print()
            } else {
                MediaType::screen()
            },
            viewport_size: euclid::Size2D::new(env.viewport_width, env.viewport_height),
            device_size: euclid::Size2D::new(env.viewport_width, env.viewport_height),
            device_pixel_ratio: euclid::Scale::new(env.device_pixel_ratio),
            quirks_mode: quirks,
            prefers_color_scheme: match env.color_scheme {
                ColorScheme::Light => PrefersColorScheme::Light,
                ColorScheme::Dark => PrefersColorScheme::Dark,
            },
            prefers_reduced_motion: if env.reduced_motion {
                PrefersReducedMotion::Reduce
            } else {
                PrefersReducedMotion::NoPreference
            },
            primary_pointer_capabilities: Default::default(),
            all_pointer_capabilities: Default::default(),
        },
    )
}
