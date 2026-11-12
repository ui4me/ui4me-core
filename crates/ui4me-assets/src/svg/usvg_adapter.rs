//! owner: 007-resources (T049, T050) — the usvg/resvg adapter behind the AD-33 `SvgRenderer` seam.
//!
//! * `usvg::Options` is built field by field per request from the document's policy, never with
//!   `Options::default()` (AD-24); usvg's `text`, `svgz`, `system-fonts`, `memmap-fonts` and `writer`
//!   features are not compiled in.
//! * The policy `ImageHrefResolver`: `resolve_string` refuses every reference (files and URLs) and
//!   records one `RESOURCE_DENIED` per distinct href; `resolve_data` admits PNG, JPEG, GIF and WebP
//!   within the `data:` limit and the pixel budgets (checked with the owned header probes before
//!   resvg decodes anything), and parses a nested SVG itself with a depth counter and a shared
//!   element budget (`SIZE_LIMIT` beyond, which fails the whole load).
//! * Embedded rasters (T050, X-ACQ = PASS) are drawn by resvg's `raster-images` feature, which uses
//!   the same `png`, `gif`, `image-webp` and `zune-jpeg` versions as the `image` adapter, so no
//!   decoder is duplicated (`cargo tree -d` shows only an unrelated `miniz_oxide` pair).
//! * Rendering is static: usvg has no scripting, animation or interactivity. `currentColor`
//!   resolves inside the document; the initial colour is black.
//! * The natural size comes from the owned pre-scan, never from usvg's `default_size`.

use super::prescan;
use crate::decode::{
    DecodedImage, ImageFormat, SvgNatural, SvgOptions, SvgParsed, SvgRenderer, SvgTree, sniff,
};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// CSS default object size, used by usvg only for documents without any size or `viewBox`.
const FALLBACK_SIZE: (f32, f32) = (300.0, 150.0);

#[derive(Debug, Default)]
pub struct UsvgRenderer;

/// The tree behind an [`SvgTree`] handle.
struct ParsedTree {
    tree: usvg::Tree,
    /// No intrinsic ratio: the tree is stretched over the raster.
    fill: bool,
    /// A zero-sized `viewBox` disables rendering: the raster stays transparent.
    disabled: bool,
}

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

#[derive(Default)]
struct State {
    /// Elements of the document and of every nested document.
    elements: u64,
    embedded_pixels: u64,
    denied: HashSet<String>,
    warnings: Vec<Code>,
    fatal: Option<Status>,
}

struct Ctx {
    opts: SvgOptions,
    state: Mutex<State>,
}

impl Ctx {
    fn warn(&self, code: Code) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .warnings
            .push(code);
    }
    fn fatal(&self, s: Status) {
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.fatal.get_or_insert(s);
    }
}

fn kind_of(format: ImageFormat, data: Arc<Vec<u8>>) -> Option<usvg::ImageKind> {
    match format {
        ImageFormat::Png => Some(usvg::ImageKind::PNG(data)),
        ImageFormat::Jpeg => Some(usvg::ImageKind::JPEG(data)),
        ImageFormat::Gif => Some(usvg::ImageKind::GIF(data)),
        ImageFormat::WebP => Some(usvg::ImageKind::WEBP(data)),
        ImageFormat::Bmp | ImageFormat::Fake => None,
    }
}

fn resolve_data(
    ctx: &Arc<Ctx>,
    depth: u32,
    mime: &str,
    data: Arc<Vec<u8>>,
) -> Option<usvg::ImageKind> {
    if data.len() as u64 > ctx.opts.max_data_bytes {
        ctx.warn(Code::SizeLimit);
        return None;
    }
    if let Some(format) = sniff::sniff_raster(&data) {
        // The signature decides; a declared type that disagrees is ignored (as for files).
        let header = match sniff::probe_header(format, &data) {
            Ok(h) => h,
            Err(_) => {
                ctx.warn(Code::DecodeFailed);
                return None;
            }
        };
        let pixels = header.width as u64 * header.height as u64;
        let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        if pixels > ctx.opts.max_embedded_pixels
            || st.embedded_pixels + pixels > ctx.opts.max_embedded_pixels
        {
            st.warnings.push(Code::SizeLimit);
            return None;
        }
        let Some(kind) = kind_of(format, data) else {
            st.warnings.push(Code::UnsupportedMediaType);
            return None;
        };
        st.embedded_pixels += pixels;
        return Some(kind);
    }
    if !(mime == "image/svg+xml" || mime == "text/plain" || mime.is_empty()) {
        ctx.warn(Code::UnsupportedMediaType);
        return None;
    }
    // Nested SVG: parsed here with a depth counter and the shared element budget.
    if depth + 1 > ctx.opts.max_nesting {
        ctx.fatal(err(
            Code::SizeLimit,
            format!(
                "data: SVG images nest deeper than {} levels",
                ctx.opts.max_nesting
            ),
        ));
        return None;
    }
    let remaining = {
        let st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        (ctx.opts.max_nodes as u64).saturating_sub(st.elements)
    };
    let scan = match prescan::scan(&data, remaining.min(u32::MAX as u64) as u32) {
        Ok(s) => s,
        Err(s) if s.code == Code::SizeLimit => {
            ctx.fatal(s);
            return None;
        }
        Err(_) => {
            ctx.warn(Code::DecodeFailed);
            return None;
        }
    };
    ctx.state.lock().unwrap_or_else(|p| p.into_inner()).elements += scan.elements as u64;
    let opts = make_options(ctx, depth + 1);
    match usvg::Tree::from_data(&data, &opts) {
        Ok(tree) => Some(usvg::ImageKind::SVG(tree)),
        Err(_) => {
            ctx.warn(Code::DecodeFailed);
            None
        }
    }
}

fn resolve_string(ctx: &Arc<Ctx>, href: &str) -> Option<usvg::ImageKind> {
    let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    if st.denied.insert(href.to_string()) {
        st.warnings.push(Code::ResourceDenied);
    }
    None
}

/// The options of one parse level; every field is set explicitly (never `Options::default()`).
fn make_options(ctx: &Arc<Ctx>, depth: u32) -> usvg::Options<'static> {
    let (cd, cs) = (ctx.clone(), ctx.clone());
    usvg::Options {
        resources_dir: None,
        dpi: 96.0,
        font_family: "sans-serif".to_string(),
        font_size: 16.0,
        languages: vec!["en".to_string()],
        shape_rendering: usvg::ShapeRendering::GeometricPrecision,
        text_rendering: usvg::TextRendering::OptimizeLegibility,
        image_rendering: usvg::ImageRendering::OptimizeQuality,
        default_size: usvg::Size::from_wh(FALLBACK_SIZE.0, FALLBACK_SIZE.1).expect("positive size"),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, _| resolve_data(&cd, depth, mime, data)),
            resolve_string: Box::new(move |href, _| resolve_string(&cs, href)),
        },
        style_sheet: None,
    }
}

fn map_usvg_error(e: usvg::Error) -> Status {
    match e {
        usvg::Error::ElementsLimitReached => {
            err(Code::SizeLimit, "SVG exceeds usvg's element limit")
        }
        usvg::Error::SvgzFeatureNotEnabled | usvg::Error::MalformedGZip => err(
            Code::UnsupportedMediaType,
            "compressed SVG (SVGZ) is unsupported",
        ),
        usvg::Error::NotAnUtf8Str => err(Code::DecodeFailed, "SVG is not UTF-8"),
        usvg::Error::InvalidSize => err(Code::DecodeFailed, "SVG has an invalid size"),
        usvg::Error::ParsingFailed(e) => {
            err(Code::DecodeFailed, format!("SVG is not well-formed: {e}"))
        }
    }
}

impl SvgRenderer for UsvgRenderer {
    fn parse(&self, bytes: &[u8], options: &SvgOptions) -> Result<SvgParsed, Status> {
        let pre = prescan::scan(bytes, options.max_nodes)?;
        let ctx = Arc::new(Ctx {
            opts: options.clone(),
            state: Mutex::new(State {
                elements: pre.elements as u64,
                ..Default::default()
            }),
        });
        let tree = usvg::Tree::from_data(bytes, &make_options(&ctx, 0)).map_err(map_usvg_error)?;
        let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(f) = st.fatal.take() {
            return Err(f);
        }
        let warnings = std::mem::take(&mut st.warnings);
        drop(st);
        let mut reports = Vec::new();
        if pre.has_text {
            reports.push(super::TEXT_KEY);
        }
        // A document without a `viewBox` and without both dimensions has no intrinsic ratio: its
        // content is laid out against the box it is drawn into (SVG 2 "width/height auto", CSS
        // Images 3 section 5.1), so it fills the raster instead of being letterboxed.
        let fill = pre.natural.ratio.is_none() || pre.stretch;
        Ok(SvgParsed {
            tree: Arc::new(ParsedTree {
                tree,
                fill,
                disabled: pre.disabled,
            }) as SvgTree,
            natural: pre.natural,
            reports,
            warnings,
        })
    }

    fn rasterize(
        &self,
        tree: &SvgTree,
        width_px: u32,
        height_px: u32,
    ) -> Result<DecodedImage, Status> {
        let parsed = tree
            .downcast_ref::<ParsedTree>()
            .ok_or_else(|| err(Code::InvalidArgument, "not a usvg tree"))?;
        let tree = &parsed.tree;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width_px, height_px).ok_or_else(|| {
            err(
                Code::SizeLimit,
                "cannot rasterise at a zero or oversized size",
            )
        })?;
        let size = tree.size();
        // Default preserveAspectRatio (xMidYMid meet): the document is fitted into the used box.
        let (sx, sy) = (
            width_px as f32 / size.width(),
            height_px as f32 / size.height(),
        );
        let transform = if parsed.fill {
            resvg::tiny_skia::Transform::from_row(sx, 0.0, 0.0, sy, 0.0, 0.0)
        } else {
            let s = sx.min(sy);
            let tx = (width_px as f32 - size.width() * s) / 2.0;
            let ty = (height_px as f32 - size.height() * s) / 2.0;
            resvg::tiny_skia::Transform::from_row(s, 0.0, 0.0, s, tx, ty)
        };
        if !parsed.disabled {
            resvg::render(tree, transform, &mut pixmap.as_mut());
        }
        Ok(DecodedImage {
            width: width_px,
            height: height_px,
            data: pixmap.take(),
        })
    }
}

/// The natural size an adapter-less caller would see (kept for tests).
pub fn natural_of(bytes: &[u8]) -> Result<SvgNatural, Status> {
    Ok(prescan::scan(bytes, 1_000_000)?.natural)
}
