//! owner: 006-style-engine — lane L-STY-r2 (T038/T039 subset: the `paint` and `overflow` groups)
//!
//! Typed paint-facing groups of `ComputedStyleView` (006 `computed-style-view.md` §3): background
//! layers (image incl. gradients and `url()`, size, position, repeat, origin, clip, attachment,
//! blend mode), `box-shadow`, `outline`, `transform` (+ `translate`/`rotate`/`scale`, origin,
//! perspective, `transform-style`, `backface-visibility`), `filter`, `mix-blend-mode` and the
//! `overflow` group. No engine (Stylo) type appears here. Lengths are computed CSS px, angles are
//! degrees, percentages are fractions (`0.5` = 50 %), `calc()` is a [`CalcHandle`], colours are
//! resolved (`currentcolor` replaced by the element's `color`).
//!
//! Anything the view cannot type is carried as an explicit `Unsupported(<what>)` variant (never
//! approximated) so that the consumer can report it.

use super::groups::{BorderStyle, Color, ColorSpace, LengthPercentage, color, css, lp};
use style::properties::ComputedValues;
use style::values::computed::image::{
    Gradient as StyloGradient, Image as StyloImage, LineDirection,
};
use style::values::generics::NonNegative;
use style::values::generics::image::{
    GenericCircle, GenericEllipse, GenericEndingShape, GenericGradientItem, GradientCompatMode,
    GradientFlags, ShapeExtent,
};

// ------------------------------------------------------------------ images

/// A CSS `url()` as the view knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssUrl {
    /// The absolute URL when the engine could resolve it (`data:` URLs, absolute URLs); otherwise
    /// the reference exactly as written (resolve it against the document or sheet base).
    pub text: String,
    /// Whether `text` is an absolute URL.
    pub resolved: bool,
}

/// How a gradient interpolates colours (CSS Images 4 `<color-interpolation-method>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientInterpolation {
    /// The legacy default of the gradient syntax (premultiplied sRGB): no `in <space>` was written.
    pub is_default: bool,
    pub space: ColorSpace,
    /// The hue interpolation keyword for polar spaces: `shorter`, `longer`, `increasing`,
    /// `decreasing` or `specified`.
    pub hue: &'static str,
}

/// A colour stop or an interpolation hint of a linear or radial gradient.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientItem {
    Stop {
        color: Color,
        position: Option<LengthPercentage>,
    },
    /// A colour hint (midpoint) between the neighbouring stops.
    Hint(LengthPercentage),
}

/// A conic stop position: an angle (degrees) or a fraction of the turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConicPosition {
    Degrees(f32),
    /// A fraction: `25%` is `Fraction(0.25)`.
    Fraction(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConicItem {
    Stop {
        color: Color,
        position: Option<ConicPosition>,
    },
    Hint(ConicPosition),
}

/// `linear-gradient()` direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearDirection {
    /// Degrees, CSS convention (0 = to top, 90 = to right). `to <side>` forms are angles.
    Angle(f32),
    /// `to <corner>`: horizontal sign (`+1` right, `-1` left), vertical sign (`-1` top, `+1` bottom).
    Corner { x: i8, y: i8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadialExtent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RadialShape {
    Circle(f32),
    CircleExtent(RadialExtent),
    Ellipse(LengthPercentage, LengthPercentage),
    EllipseExtent(RadialExtent),
}

/// Centre or `at <position>` of a radial or conic gradient: `(x, y)` against the gradient box.
pub type GradientPosition = (LengthPercentage, LengthPercentage);

#[derive(Clone, Debug, PartialEq)]
pub enum Gradient {
    Linear {
        direction: LinearDirection,
        items: Vec<GradientItem>,
        repeating: bool,
        interpolation: GradientInterpolation,
    },
    Radial {
        shape: RadialShape,
        position: GradientPosition,
        items: Vec<GradientItem>,
        repeating: bool,
        interpolation: GradientInterpolation,
    },
    Conic {
        from_deg: f32,
        position: GradientPosition,
        items: Vec<ConicItem>,
        repeating: bool,
        interpolation: GradientInterpolation,
    },
}

/// A computed `<image>`.
#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    None,
    Url(CssUrl),
    Gradient(Gradient),
    /// A form the view does not type (`cross-fade()`, `image(<color>)`, paint worklets, prefixed
    /// gradient syntaxes, `contain`/`cover` radial extents, ...). The consumer reports it.
    Unsupported(&'static str),
}

// ------------------------------------------------------------------ background layers

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundBox {
    BorderBox,
    PaddingBox,
    ContentBox,
    /// `background-clip: text` (also `-webkit-background-clip: text`).
    Text,
    /// `background-clip: border-area`.
    BorderArea,
    /// `no-clip` and other values the view does not type.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundRepeat {
    Repeat,
    Space,
    Round,
    NoRepeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundAttachment {
    Scroll,
    Fixed,
    Local,
}

/// One `background-size` axis.
#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundSizeAxis {
    Auto,
    LengthPercentage(LengthPercentage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundSize {
    Cover,
    Contain,
    Explicit {
        width: BackgroundSizeAxis,
        height: BackgroundSizeAxis,
    },
}

/// One `background-*` layer (the layer count is the number of `background-image` values; the other
/// lists are repeated or truncated to it, CSS Backgrounds 3 section 2.1).
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundLayer {
    pub image: Image,
    /// `background-position-x/-y` as computed length-percentages (keywords are percentages).
    pub position: (LengthPercentage, LengthPercentage),
    pub size: BackgroundSize,
    pub repeat: (BackgroundRepeat, BackgroundRepeat),
    pub attachment: BackgroundAttachment,
    pub origin: BackgroundBox,
    pub clip: BackgroundBox,
    /// `background-blend-mode` keyword (`normal`, `multiply`, ...).
    pub blend_mode: &'static str,
}

// ------------------------------------------------------------------ mask layers

/// `mask-mode` of a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskMode {
    MatchSource,
    Alpha,
    Luminance,
}

/// `mask-composite` of a layer (CSS Masking 1 section 7.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskComposite {
    Add,
    Subtract,
    Intersect,
    Exclude,
}

/// One `mask-*` layer (the layer count is the number of `mask-image` values; the other lists are
/// repeated or truncated to it, like the background layers). `origin` and `clip` use the
/// background boxes (`mask-clip: no-clip` and the SVG boxes are not parsed by this engine).
#[derive(Clone, Debug, PartialEq)]
pub struct MaskLayer {
    pub image: Image,
    pub mode: MaskMode,
    pub position: (LengthPercentage, LengthPercentage),
    pub size: BackgroundSize,
    pub repeat: (BackgroundRepeat, BackgroundRepeat),
    pub origin: BackgroundBox,
    pub clip: BackgroundBox,
    pub composite: MaskComposite,
}

// ------------------------------------------------------------------ shadows, outline, filter

/// One `box-shadow` layer, first layer topmost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

/// One `text-shadow` layer, first layer topmost (CSS Text Decoration 3 section 4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineStyle {
    Auto,
    Style(BorderStyle),
}

/// `outline-*` (`outline-width` is snapped as a border width, spec FR-077; 0 when the style is `none`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    pub width: f32,
    pub style: OutlineStyle,
    pub color: Color,
    /// The computed `outline-color` is `currentcolor` (the initial value; `outline: auto` resets to it).
    pub color_is_current: bool,
    pub offset: f32,
}

/// One `filter` function (the `url()` form is excluded by the profile).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterFunction {
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    HueRotate(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    DropShadow {
        offset_x: f32,
        offset_y: f32,
        blur: f32,
        color: Color,
    },
}

// ------------------------------------------------------------------ transform

/// One function of the computed `transform` list (lengths in px, angles in degrees).
#[derive(Clone, Debug, PartialEq)]
pub enum TransformFunction {
    /// `matrix(a, b, c, d, e, f)`.
    Matrix([f32; 6]),
    /// `matrix3d(...)`, in CSS order (column-major: `m11, m12, m13, m14, m21, ...`).
    Matrix3d([f32; 16]),
    Translate {
        x: LengthPercentage,
        y: LengthPercentage,
        z: f32,
    },
    Scale {
        x: f32,
        y: f32,
        z: f32,
    },
    /// Rotation about the axis `(x, y, z)` (`rotate()` is `(0, 0, 1)`).
    Rotate {
        x: f32,
        y: f32,
        z: f32,
        deg: f32,
    },
    Skew {
        x_deg: f32,
        y_deg: f32,
    },
    /// `perspective(<length>)`; `None` for `perspective(none)`.
    Perspective(Option<f32>),
    /// A transition intermediate (`interpolatematrix` / `accumulatematrix`): never produced from
    /// author CSS; carried so that the consumer can report it.
    Intermediate,
}

/// `translate`, `rotate`, `scale` (individual transform properties).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct IndividualTransform {
    pub translate: Option<(LengthPercentage, LengthPercentage, f32)>,
    /// Axis and angle in degrees.
    pub rotate: Option<([f32; 3], f32)>,
    pub scale: Option<[f32; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransformOrigin {
    pub x: LengthPercentage,
    pub y: LengthPercentage,
    pub z: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transform {
    pub list: Vec<TransformFunction>,
    pub individual: IndividualTransform,
    pub origin: TransformOrigin,
    /// `perspective` of this element (applies to its children); `None` = `none`.
    pub perspective: Option<f32>,
    pub preserve_3d: bool,
    pub backface_hidden: bool,
}

impl Transform {
    /// Whether the element has any transform property set (list or individual).
    pub fn is_none(&self) -> bool {
        self.list.is_empty() && self.individual == IndividualTransform::default()
    }
}

// ------------------------------------------------------------------ overflow

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverflowKind {
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

impl OverflowKind {
    /// Whether the value clips the padding box (`visible` does not).
    pub fn clips(self) -> bool {
        self != OverflowKind::Visible
    }
}

/// The `overflow` group (computed; `visible` on one axis with a clipping value on the other is
/// already computed to `auto` by the engine).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overflow {
    pub x: OverflowKind,
    pub y: OverflowKind,
}

// ------------------------------------------------------------------ extraction

fn lp_stop(epoch: u64, v: &style::values::computed::LengthPercentage) -> LengthPercentage {
    lp(epoch, v)
}

fn interpolation(
    m: &style::color::mix::ColorInterpolationMethod,
    flags: GradientFlags,
) -> GradientInterpolation {
    use style::color::ColorSpace as S;
    use style::color::mix::HueInterpolationMethod as H;
    let space = match m.space {
        S::Srgb => ColorSpace::Srgb,
        S::Hsl => ColorSpace::Hsl,
        S::Hwb => ColorSpace::Hwb,
        S::Lab => ColorSpace::Lab,
        S::Lch => ColorSpace::Lch,
        S::Oklab => ColorSpace::Oklab,
        S::Oklch => ColorSpace::Oklch,
        S::SrgbLinear => ColorSpace::SrgbLinear,
        S::DisplayP3 | S::DisplayP3Linear => ColorSpace::DisplayP3,
        S::A98Rgb => ColorSpace::A98,
        S::ProphotoRgb => ColorSpace::ProPhoto,
        S::Rec2020 => ColorSpace::Rec2020,
        S::XyzD50 => ColorSpace::XyzD50,
        S::XyzD65 => ColorSpace::XyzD65,
    };
    let hue = match m.hue {
        H::Shorter => "shorter",
        H::Longer => "longer",
        H::Increasing => "increasing",
        H::Decreasing => "decreasing",
        H::Specified => "specified",
    };
    GradientInterpolation {
        is_default: flags.contains(GradientFlags::HAS_DEFAULT_COLOR_INTERPOLATION_METHOD),
        space,
        hue,
    }
}

fn extent(e: ShapeExtent) -> Option<RadialExtent> {
    Some(match e {
        ShapeExtent::ClosestSide => RadialExtent::ClosestSide,
        ShapeExtent::FarthestSide => RadialExtent::FarthestSide,
        ShapeExtent::ClosestCorner => RadialExtent::ClosestCorner,
        ShapeExtent::FarthestCorner => RadialExtent::FarthestCorner,
        // Legacy (prefixed-syntax) keywords.
        ShapeExtent::Contain | ShapeExtent::Cover => return None,
    })
}

fn linear_items(
    cv: &ComputedValues,
    epoch: u64,
    items: &[GenericGradientItem<
        style::values::computed::Color,
        style::values::computed::LengthPercentage,
    >],
) -> Vec<GradientItem> {
    items
        .iter()
        .map(|i| match i {
            GenericGradientItem::SimpleColorStop(c) => GradientItem::Stop {
                color: color(cv.resolve_color(c)),
                position: None,
            },
            GenericGradientItem::ComplexColorStop { color: c, position } => GradientItem::Stop {
                color: color(cv.resolve_color(c)),
                position: Some(lp_stop(epoch, position)),
            },
            GenericGradientItem::InterpolationHint(p) => GradientItem::Hint(lp_stop(epoch, p)),
        })
        .collect()
}

fn conic_position(p: &style::values::computed::AngleOrPercentage) -> ConicPosition {
    match p {
        style::values::computed::AngleOrPercentage::Angle(a) => ConicPosition::Degrees(a.degrees()),
        style::values::computed::AngleOrPercentage::Percentage(p) => ConicPosition::Fraction(p.0),
    }
}

fn gradient(cv: &ComputedValues, epoch: u64, g: &StyloGradient) -> Image {
    let pos =
        |p: &style::values::computed::Position| (lp(epoch, &p.horizontal), lp(epoch, &p.vertical));
    match g {
        StyloGradient::Linear {
            direction,
            color_interpolation_method,
            items,
            flags,
            compat_mode,
        } => {
            if *compat_mode != GradientCompatMode::Modern {
                return Image::Unsupported("prefixed-gradient");
            }
            use style::values::specified::position::{
                HorizontalPositionKeyword as Hk, VerticalPositionKeyword as Vk,
            };
            let direction = match direction {
                LineDirection::Angle(a) => LinearDirection::Angle(a.degrees()),
                LineDirection::Horizontal(Hk::Left) => LinearDirection::Angle(270.0),
                LineDirection::Horizontal(Hk::Right) => LinearDirection::Angle(90.0),
                LineDirection::Vertical(Vk::Top) => LinearDirection::Angle(0.0),
                LineDirection::Vertical(Vk::Bottom) => LinearDirection::Angle(180.0),
                LineDirection::Corner(h, v) => LinearDirection::Corner {
                    x: if *h == Hk::Right { 1 } else { -1 },
                    y: if *v == Vk::Bottom { 1 } else { -1 },
                },
            };
            Image::Gradient(Gradient::Linear {
                direction,
                items: linear_items(cv, epoch, items),
                repeating: flags.contains(GradientFlags::REPEATING),
                interpolation: interpolation(color_interpolation_method, *flags),
            })
        }
        StyloGradient::Radial {
            shape,
            position,
            color_interpolation_method,
            items,
            flags,
            compat_mode,
        } => {
            if *compat_mode != GradientCompatMode::Modern {
                return Image::Unsupported("prefixed-gradient");
            }
            let shape = match shape {
                GenericEndingShape::Circle(GenericCircle::Radius(NonNegative(r))) => {
                    RadialShape::Circle(r.px())
                }
                GenericEndingShape::Circle(GenericCircle::Extent(e)) => match extent(*e) {
                    Some(e) => RadialShape::CircleExtent(e),
                    None => return Image::Unsupported("prefixed-gradient"),
                },
                GenericEndingShape::Ellipse(GenericEllipse::Radii(
                    NonNegative(rx),
                    NonNegative(ry),
                )) => RadialShape::Ellipse(lp(epoch, rx), lp(epoch, ry)),
                GenericEndingShape::Ellipse(GenericEllipse::Extent(e)) => match extent(*e) {
                    Some(e) => RadialShape::EllipseExtent(e),
                    None => return Image::Unsupported("prefixed-gradient"),
                },
            };
            Image::Gradient(Gradient::Radial {
                shape,
                position: pos(position),
                items: linear_items(cv, epoch, items),
                repeating: flags.contains(GradientFlags::REPEATING),
                interpolation: interpolation(color_interpolation_method, *flags),
            })
        }
        StyloGradient::Conic {
            angle,
            position,
            color_interpolation_method,
            items,
            flags,
        } => {
            let items = items
                .iter()
                .map(|i| match i {
                    GenericGradientItem::SimpleColorStop(c) => ConicItem::Stop {
                        color: color(cv.resolve_color(c)),
                        position: None,
                    },
                    GenericGradientItem::ComplexColorStop { color: c, position } => {
                        ConicItem::Stop {
                            color: color(cv.resolve_color(c)),
                            position: Some(conic_position(position)),
                        }
                    }
                    GenericGradientItem::InterpolationHint(p) => ConicItem::Hint(conic_position(p)),
                })
                .collect();
            Image::Gradient(Gradient::Conic {
                from_deg: angle.degrees(),
                position: pos(position),
                items,
                repeating: flags.contains(GradientFlags::REPEATING),
                interpolation: interpolation(color_interpolation_method, *flags),
            })
        }
    }
}

pub(crate) fn image(cv: &ComputedValues, epoch: u64, img: &StyloImage) -> Image {
    match img {
        StyloImage::None => Image::None,
        StyloImage::Url(u) => match u {
            style::url::ComputedUrl::Valid(url) => Image::Url(CssUrl {
                text: url.as_str().to_string(),
                resolved: true,
            }),
            style::url::ComputedUrl::Invalid(text) => Image::Url(CssUrl {
                text: text.to_string(),
                resolved: false,
            }),
        },
        StyloImage::Gradient(g) => gradient(cv, epoch, g),
        StyloImage::ImageSet(set) => match set.items.get(set.selected_index) {
            Some(item) => image(cv, epoch, &item.image),
            None => Image::Unsupported("image-set"),
        },
        StyloImage::LightDark(ld) => image(cv, epoch, &ld.light),
        StyloImage::CrossFade(_) => Image::Unsupported("cross-fade"),
        StyloImage::PaintWorklet(_) => Image::Unsupported("paint-worklet"),
        StyloImage::Image(_) => Image::Unsupported("image-function"),
    }
}

fn repeat_kw(k: style::values::specified::background::BackgroundRepeatKeyword) -> BackgroundRepeat {
    use style::values::specified::background::BackgroundRepeatKeyword as K;
    match k {
        K::Repeat => BackgroundRepeat::Repeat,
        K::Space => BackgroundRepeat::Space,
        K::Round => BackgroundRepeat::Round,
        K::NoRepeat => BackgroundRepeat::NoRepeat,
    }
}

fn bg_box(keyword: &str) -> BackgroundBox {
    match keyword {
        "border-box" => BackgroundBox::BorderBox,
        "padding-box" => BackgroundBox::PaddingBox,
        "content-box" => BackgroundBox::ContentBox,
        "text" => BackgroundBox::Text,
        "border-area" => BackgroundBox::BorderArea,
        _ => BackgroundBox::Other,
    }
}

fn size_axis(
    epoch: u64,
    a: &style::values::generics::length::GenericLengthPercentageOrAuto<
        NonNegative<style::values::computed::LengthPercentage>,
    >,
) -> BackgroundSizeAxis {
    use style::values::generics::length::GenericLengthPercentageOrAuto as L;
    match a {
        L::Auto => BackgroundSizeAxis::Auto,
        L::LengthPercentage(v) => BackgroundSizeAxis::LengthPercentage(lp(epoch, &v.0)),
    }
}

fn blend_keyword(s: &str) -> &'static str {
    const KW: [&str; 16] = [
        "normal",
        "multiply",
        "screen",
        "overlay",
        "darken",
        "lighten",
        "color-dodge",
        "color-burn",
        "hard-light",
        "soft-light",
        "difference",
        "exclusion",
        "hue",
        "saturation",
        "color",
        "luminosity",
    ];
    KW.iter().copied().find(|k| *k == s).unwrap_or("unknown")
}

pub(crate) fn background_layers(cv: &ComputedValues, epoch: u64) -> Vec<BackgroundLayer> {
    let bg = cv.get_background();
    let n = bg.background_image.0.len();
    let mut out = Vec::with_capacity(n);
    // Every list repeats to the number of layers (CSS Backgrounds 3 section 2.1).
    fn cyc<T>(v: &[T], i: usize) -> Option<&T> {
        if v.is_empty() {
            None
        } else {
            Some(&v[i % v.len()])
        }
    }
    for i in 0..n {
        let image = image(cv, epoch, &bg.background_image.0[i]);
        let px = cyc(&bg.background_position_x.0, i)
            .map(|v| lp(epoch, v))
            .unwrap_or(LengthPercentage::Percent(0.0));
        let py = cyc(&bg.background_position_y.0, i)
            .map(|v| lp(epoch, v))
            .unwrap_or(LengthPercentage::Percent(0.0));
        let size = match cyc(&bg.background_size.0, i) {
            Some(style::values::generics::background::GenericBackgroundSize::Cover) => {
                BackgroundSize::Cover
            }
            Some(style::values::generics::background::GenericBackgroundSize::Contain) => {
                BackgroundSize::Contain
            }
            Some(style::values::generics::background::GenericBackgroundSize::ExplicitSize {
                width,
                height,
            }) => BackgroundSize::Explicit {
                width: size_axis(epoch, width),
                height: size_axis(epoch, height),
            },
            None => BackgroundSize::Explicit {
                width: BackgroundSizeAxis::Auto,
                height: BackgroundSizeAxis::Auto,
            },
        };
        let repeat = cyc(&bg.background_repeat.0, i)
            .map(|r| (repeat_kw(r.0), repeat_kw(r.1)))
            .unwrap_or((BackgroundRepeat::Repeat, BackgroundRepeat::Repeat));
        let attachment = match cyc(&bg.background_attachment.0, i).map(css).as_deref() {
            Some("fixed") => BackgroundAttachment::Fixed,
            Some("local") => BackgroundAttachment::Local,
            _ => BackgroundAttachment::Scroll,
        };
        let origin = cyc(&bg.background_origin.0, i)
            .map(|o| bg_box(&css(o)))
            .unwrap_or(BackgroundBox::PaddingBox);
        let clip = cyc(&bg.background_clip.0, i)
            .map(|o| bg_box(&css(o)))
            .unwrap_or(BackgroundBox::BorderBox);
        let blend_mode = cyc(&bg.background_blend_mode.0, i)
            .map(|b| blend_keyword(&css(b)))
            .unwrap_or("normal");
        out.push(BackgroundLayer {
            image,
            position: (px, py),
            size,
            repeat,
            attachment,
            origin,
            clip,
            blend_mode,
        });
    }
    out
}

/// The `mask-*` layers, first layer topmost (`mask-image` count; other lists cycle).
pub(crate) fn mask_layers(cv: &ComputedValues, epoch: u64) -> Vec<MaskLayer> {
    let m = cv.get_svg();
    let n = m.mask_image.0.len();
    let mut out = Vec::with_capacity(n);
    fn cyc<T>(v: &[T], i: usize) -> Option<&T> {
        if v.is_empty() {
            None
        } else {
            Some(&v[i % v.len()])
        }
    }
    for i in 0..n {
        let image = image(cv, epoch, &m.mask_image.0[i]);
        let px = cyc(&m.mask_position_x.0, i)
            .map(|v| lp(epoch, v))
            .unwrap_or(LengthPercentage::Percent(0.0));
        let py = cyc(&m.mask_position_y.0, i)
            .map(|v| lp(epoch, v))
            .unwrap_or(LengthPercentage::Percent(0.0));
        let size = match cyc(&m.mask_size.0, i) {
            Some(style::values::generics::background::GenericBackgroundSize::Cover) => {
                BackgroundSize::Cover
            }
            Some(style::values::generics::background::GenericBackgroundSize::Contain) => {
                BackgroundSize::Contain
            }
            Some(style::values::generics::background::GenericBackgroundSize::ExplicitSize {
                width,
                height,
            }) => BackgroundSize::Explicit {
                width: size_axis(epoch, width),
                height: size_axis(epoch, height),
            },
            None => BackgroundSize::Explicit {
                width: BackgroundSizeAxis::Auto,
                height: BackgroundSizeAxis::Auto,
            },
        };
        let repeat = cyc(&m.mask_repeat.0, i)
            .map(|r| (repeat_kw(r.0), repeat_kw(r.1)))
            .unwrap_or((BackgroundRepeat::Repeat, BackgroundRepeat::Repeat));
        let origin = cyc(&m.mask_origin.0, i)
            .map(|o| bg_box(&css(o)))
            .unwrap_or(BackgroundBox::BorderBox);
        let clip = cyc(&m.mask_clip.0, i)
            .map(|o| bg_box(&css(o)))
            .unwrap_or(BackgroundBox::BorderBox);
        let mode = match cyc(&m.mask_mode.0, i).map(css).as_deref() {
            Some("alpha") => MaskMode::Alpha,
            Some("luminance") => MaskMode::Luminance,
            _ => MaskMode::MatchSource,
        };
        let composite = match cyc(&m.mask_composite.0, i).map(css).as_deref() {
            Some("subtract") => MaskComposite::Subtract,
            Some("intersect") => MaskComposite::Intersect,
            Some("exclude") => MaskComposite::Exclude,
            _ => MaskComposite::Add,
        };
        out.push(MaskLayer {
            image,
            mode,
            position: (px, py),
            size,
            repeat,
            origin,
            clip,
            composite,
        });
    }
    out
}

// ------------------------------------------------------------------ border-image

/// One `border-image-slice` offset: a number of image pixels or a fraction of the image size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderImageSlice {
    Number(f32),
    /// `25%` is `Fraction(0.25)`.
    Fraction(f32),
}

/// One `border-image-width` side.
#[derive(Clone, Debug, PartialEq)]
pub enum BorderImageWidth {
    /// A multiple of the matching `border-*-width`.
    Number(f32),
    LengthPercentage(LengthPercentage),
    /// The intrinsic size of the matching slice.
    Auto,
}

/// One `border-image-outset` side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderImageOutset {
    /// A multiple of the matching `border-*-width`.
    Number(f32),
    Px(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderImageRepeat {
    Stretch,
    Repeat,
    Round,
    Space,
}

/// The `border-image-*` longhands of an element whose `border-image-source` is not `none`
/// (CSS Backgrounds 3 section 6). Sides are in the order top, right, bottom, left.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderImage {
    pub source: Image,
    pub slice: [BorderImageSlice; 4],
    pub fill: bool,
    pub width: [BorderImageWidth; 4],
    pub outset: [BorderImageOutset; 4],
    /// Horizontal, vertical.
    pub repeat: (BorderImageRepeat, BorderImageRepeat),
}

pub(crate) fn border_image(cv: &ComputedValues, epoch: u64) -> Option<BorderImage> {
    use style::values::computed::NumberOrPercentage as Nop;
    use style::values::generics::border::BorderImageSideWidth as W;
    use style::values::generics::length::GenericLengthOrNumber as LoN;
    use style::values::specified::border::BorderImageRepeatKeyword as K;
    let b = cv.get_border();
    if matches!(b.border_image_source, StyloImage::None) {
        return None;
    }
    let source = image(cv, epoch, &b.border_image_source);
    let slice_of = |v: &style::values::computed::NonNegativeNumberOrPercentage| match v.0 {
        Nop::Number(n) => BorderImageSlice::Number(n),
        Nop::Percentage(p) => BorderImageSlice::Fraction(p.0),
    };
    let sl = &b.border_image_slice.offsets;
    let w = &b.border_image_width;
    let width_of = |v: &style::values::computed::BorderImageSideWidth| match v {
        W::Number(n) => BorderImageWidth::Number(n.0),
        W::LengthPercentage(l) => BorderImageWidth::LengthPercentage(lp(epoch, &l.0)),
        W::Auto => BorderImageWidth::Auto,
    };
    let o = &b.border_image_outset;
    let outset_of = |v: &style::values::computed::NonNegativeLengthOrNumber| match v {
        LoN::Number(n) => BorderImageOutset::Number(n.0),
        LoN::Length(l) => BorderImageOutset::Px(l.0.px()),
    };
    let kw = |k: K| match k {
        K::Stretch => BorderImageRepeat::Stretch,
        K::Repeat => BorderImageRepeat::Repeat,
        K::Round => BorderImageRepeat::Round,
        K::Space => BorderImageRepeat::Space,
    };
    Some(BorderImage {
        source,
        slice: [
            slice_of(&sl.0),
            slice_of(&sl.1),
            slice_of(&sl.2),
            slice_of(&sl.3),
        ],
        fill: b.border_image_slice.fill,
        width: [
            width_of(&w.0),
            width_of(&w.1),
            width_of(&w.2),
            width_of(&w.3),
        ],
        outset: [
            outset_of(&o.0),
            outset_of(&o.1),
            outset_of(&o.2),
            outset_of(&o.3),
        ],
        repeat: (kw(b.border_image_repeat.0), kw(b.border_image_repeat.1)),
    })
}

pub(crate) fn box_shadows(cv: &ComputedValues) -> Vec<BoxShadow> {
    cv.get_effects()
        .box_shadow
        .0
        .iter()
        .map(|s| BoxShadow {
            offset_x: s.base.horizontal.px(),
            offset_y: s.base.vertical.px(),
            blur: s.base.blur.0.px(),
            spread: s.spread.px(),
            color: color(cv.resolve_color(&s.base.color)),
            inset: s.inset,
        })
        .collect()
}

pub(crate) fn text_shadows(cv: &ComputedValues) -> Vec<TextShadow> {
    cv.get_inherited_text()
        .text_shadow
        .0
        .iter()
        .map(|s| TextShadow {
            offset_x: s.horizontal.px(),
            offset_y: s.vertical.px(),
            blur: s.blur.px(),
            color: color(cv.resolve_color(&s.color)),
        })
        .collect()
}

pub(crate) fn outline(cv: &ComputedValues) -> Outline {
    use style::values::computed::OutlineStyle as O;
    let o = cv.get_outline();
    let style = match &o.outline_style {
        O::Auto => OutlineStyle::Auto,
        O::BorderStyle(b) => OutlineStyle::Style(super::groups::border_style_of(*b)),
    };
    // CSS UI 4: `outline-width` computes to 0 when the outline style is `none`.
    let width = if matches!(style, OutlineStyle::Style(BorderStyle::None)) {
        0.0
    } else {
        o.outline_width.0.to_f32_px()
    };
    Outline {
        width,
        style,
        color: color(cv.resolve_color(&o.outline_color)),
        color_is_current: o.outline_color.is_currentcolor(),
        offset: o.outline_offset.to_f32_px(),
    }
}

pub(crate) fn filters(cv: &ComputedValues) -> Vec<FilterFunction> {
    use style::values::generics::effects::GenericFilter as F;
    cv.get_effects()
        .filter
        .0
        .iter()
        .filter_map(|f| {
            Some(match f {
                F::Blur(v) => FilterFunction::Blur(v.0.px()),
                F::Brightness(v) => FilterFunction::Brightness(v.0),
                F::Contrast(v) => FilterFunction::Contrast(v.0),
                F::Grayscale(v) => FilterFunction::Grayscale(v.0),
                F::HueRotate(a) => FilterFunction::HueRotate(a.degrees()),
                F::Invert(v) => FilterFunction::Invert(v.0),
                F::Opacity(v) => FilterFunction::Opacity(v.0),
                F::Saturate(v) => FilterFunction::Saturate(v.0),
                F::Sepia(v) => FilterFunction::Sepia(v.0),
                F::DropShadow(s) => FilterFunction::DropShadow {
                    offset_x: s.horizontal.px(),
                    offset_y: s.vertical.px(),
                    blur: s.blur.0.px(),
                    color: color(cv.resolve_color(&s.color)),
                },
                // `filter: url(#id)` is excluded by the profile (the servo build has no such value).
                F::Url(_) => return None,
            })
        })
        .collect()
}

pub(crate) fn backdrop_filters(cv: &ComputedValues) -> Vec<FilterFunction> {
    use style::values::generics::effects::GenericFilter as F;
    cv.get_effects()
        .backdrop_filter
        .0
        .iter()
        .filter_map(|f| {
            Some(match f {
                F::Blur(v) => FilterFunction::Blur(v.0.px()),
                F::Brightness(v) => FilterFunction::Brightness(v.0),
                F::Contrast(v) => FilterFunction::Contrast(v.0),
                F::Grayscale(v) => FilterFunction::Grayscale(v.0),
                F::HueRotate(a) => FilterFunction::HueRotate(a.degrees()),
                F::Invert(v) => FilterFunction::Invert(v.0),
                F::Opacity(v) => FilterFunction::Opacity(v.0),
                F::Saturate(v) => FilterFunction::Saturate(v.0),
                F::Sepia(v) => FilterFunction::Sepia(v.0),
                F::DropShadow(s) => FilterFunction::DropShadow {
                    offset_x: s.horizontal.px(),
                    offset_y: s.vertical.px(),
                    blur: s.blur.0.px(),
                    color: color(cv.resolve_color(&s.color)),
                },
                // `filter: url(#id)` is excluded by the profile (the servo build has no such value).
                F::Url(_) => return None,
            })
        })
        .collect()
}

pub(crate) fn mix_blend_mode(cv: &ComputedValues) -> &'static str {
    blend_keyword(&css(&cv.get_effects().mix_blend_mode))
}

fn transform_function(
    epoch: u64,
    op: &style::values::computed::transform::TransformOperation,
) -> TransformFunction {
    use style::values::generics::transform::GenericPerspectiveFunction as P;
    use style::values::generics::transform::GenericTransformOperation as T;
    match op {
        T::Matrix(m) => TransformFunction::Matrix([m.a, m.b, m.c, m.d, m.e, m.f]),
        T::Matrix3D(m) => TransformFunction::Matrix3d([
            m.m11, m.m12, m.m13, m.m14, m.m21, m.m22, m.m23, m.m24, m.m31, m.m32, m.m33, m.m34,
            m.m41, m.m42, m.m43, m.m44,
        ]),
        T::Skew(x, y) => TransformFunction::Skew {
            x_deg: x.degrees(),
            y_deg: y.degrees(),
        },
        T::SkewX(x) => TransformFunction::Skew {
            x_deg: x.degrees(),
            y_deg: 0.0,
        },
        T::SkewY(y) => TransformFunction::Skew {
            x_deg: 0.0,
            y_deg: y.degrees(),
        },
        T::Translate(x, y) => TransformFunction::Translate {
            x: lp(epoch, x),
            y: lp(epoch, y),
            z: 0.0,
        },
        T::TranslateX(x) => TransformFunction::Translate {
            x: lp(epoch, x),
            y: LengthPercentage::Px(0.0),
            z: 0.0,
        },
        T::TranslateY(y) => TransformFunction::Translate {
            x: LengthPercentage::Px(0.0),
            y: lp(epoch, y),
            z: 0.0,
        },
        T::TranslateZ(z) => TransformFunction::Translate {
            x: LengthPercentage::Px(0.0),
            y: LengthPercentage::Px(0.0),
            z: z.px(),
        },
        T::Translate3D(x, y, z) => TransformFunction::Translate {
            x: lp(epoch, x),
            y: lp(epoch, y),
            z: z.px(),
        },
        T::Scale(x, y) => TransformFunction::Scale {
            x: *x,
            y: *y,
            z: 1.0,
        },
        T::ScaleX(x) => TransformFunction::Scale {
            x: *x,
            y: 1.0,
            z: 1.0,
        },
        T::ScaleY(y) => TransformFunction::Scale {
            x: 1.0,
            y: *y,
            z: 1.0,
        },
        T::ScaleZ(z) => TransformFunction::Scale {
            x: 1.0,
            y: 1.0,
            z: *z,
        },
        T::Scale3D(x, y, z) => TransformFunction::Scale {
            x: *x,
            y: *y,
            z: *z,
        },
        T::Rotate(a) | T::RotateZ(a) => TransformFunction::Rotate {
            x: 0.0,
            y: 0.0,
            z: 1.0,
            deg: a.degrees(),
        },
        T::RotateX(a) => TransformFunction::Rotate {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            deg: a.degrees(),
        },
        T::RotateY(a) => TransformFunction::Rotate {
            x: 0.0,
            y: 1.0,
            z: 0.0,
            deg: a.degrees(),
        },
        T::Rotate3D(x, y, z, a) => TransformFunction::Rotate {
            x: *x,
            y: *y,
            z: *z,
            deg: a.degrees(),
        },
        T::Perspective(P::None) => TransformFunction::Perspective(None),
        T::Perspective(P::Length(l)) => TransformFunction::Perspective(Some(l.px())),
        T::InterpolateMatrix { .. } | T::AccumulateMatrix { .. } => TransformFunction::Intermediate,
    }
}

pub(crate) fn transform(cv: &ComputedValues, epoch: u64) -> Transform {
    use style::values::generics::box_::GenericPerspective as P;
    use style::values::generics::transform::{
        GenericRotate as R, GenericScale as S, GenericTranslate as T,
    };
    let b = cv.get_box();
    let list = b
        .transform
        .0
        .iter()
        .map(|op| transform_function(epoch, op))
        .collect();
    let individual = IndividualTransform {
        translate: match &b.translate {
            T::None => None,
            T::Translate(x, y, z) => Some((lp(epoch, x), lp(epoch, y), z.px())),
        },
        rotate: match &b.rotate {
            R::None => None,
            R::Rotate(a) => Some(([0.0, 0.0, 1.0], a.degrees())),
            R::Rotate3D(x, y, z, a) => Some(([*x, *y, *z], a.degrees())),
        },
        scale: match &b.scale {
            S::None => None,
            S::Scale(x, y, z) => Some([*x, *y, *z]),
        },
    };
    let o = &b.transform_origin;
    Transform {
        list,
        individual,
        origin: TransformOrigin {
            x: lp(epoch, &o.horizontal),
            y: lp(epoch, &o.vertical),
            z: o.depth.px(),
        },
        perspective: match &b.perspective {
            P::None => None,
            P::Length(l) => Some(l.0.px()),
        },
        preserve_3d: css(&b.transform_style) == "preserve-3d",
        backface_hidden: css(&b.backface_visibility) == "hidden",
    }
}

fn overflow_kind(o: style::values::computed::Overflow) -> OverflowKind {
    use style::values::computed::Overflow as O;
    match o {
        O::Visible => OverflowKind::Visible,
        O::Hidden => OverflowKind::Hidden,
        O::Clip => OverflowKind::Clip,
        O::Scroll => OverflowKind::Scroll,
        O::Auto => OverflowKind::Auto,
    }
}

pub(crate) fn overflow(cv: &ComputedValues) -> Overflow {
    let b = cv.get_box();
    Overflow {
        x: overflow_kind(b.overflow_x),
        y: overflow_kind(b.overflow_y),
    }
}

// ------------------------------------------------------------------ clip-path

/// The reference box of a `clip-path` shape (CSS Masking 1 `<geometry-box>`). `fill-box`,
/// `stroke-box` and `view-box` map to the border box for an HTML element (010 FR-074).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipBox {
    Margin,
    Border,
    Padding,
    Content,
}

/// A `<shape-radius>` of `circle()` / `ellipse()`.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeRadius {
    Length(LengthPercentage),
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

/// One segment of a `path()` in absolute px (the origin is the reference box's top-left corner).
/// Quadratic curves and arcs are converted to cubic Beziers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    Move(f32, f32),
    Line(f32, f32),
    Cubic((f32, f32), (f32, f32), (f32, f32)),
    Close,
}

/// A `<basic-shape>` the view can type (`shape()` is not: it is `ClipPath::Unsupported`).
#[derive(Clone, Debug, PartialEq)]
pub enum BasicShape {
    /// `inset()` (and `rect()` / `xywh()`, which compute to it): insets top, right, bottom, left and
    /// the corner radii (top-left, top-right, bottom-right, bottom-left) as `(horizontal, vertical)`.
    Inset {
        insets: [LengthPercentage; 4],
        radii: [(LengthPercentage, LengthPercentage); 4],
    },
    Circle {
        radius: ShapeRadius,
        center: (LengthPercentage, LengthPercentage),
    },
    Ellipse {
        rx: ShapeRadius,
        ry: ShapeRadius,
        center: (LengthPercentage, LengthPercentage),
    },
    Polygon {
        evenodd: bool,
        points: Vec<(LengthPercentage, LengthPercentage)>,
    },
    Path {
        evenodd: bool,
        segments: Vec<PathSegment>,
    },
}

/// The computed `clip-path`.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipPath {
    None,
    /// A basic shape on a reference box.
    Shape {
        shape: Box<BasicShape>,
        reference: ClipBox,
    },
    /// A geometry box alone: the edge of that box (with its corner radii).
    Box(ClipBox),
    /// `url(...)`: removed by the profile filter before it gets here; never computed in this build.
    Url,
    /// A value the view cannot type; the feature id (`css.value.clip-path-*`) the consumer reports.
    Unsupported(&'static str),
}

fn clip_box(b: &style::values::generics::basic_shape::ShapeGeometryBox) -> ClipBox {
    use style::values::generics::basic_shape::{ShapeBox, ShapeGeometryBox as G};
    match b {
        G::ShapeBox(ShapeBox::MarginBox) => ClipBox::Margin,
        G::ShapeBox(ShapeBox::PaddingBox) => ClipBox::Padding,
        G::ShapeBox(ShapeBox::ContentBox) => ClipBox::Content,
        G::ShapeBox(ShapeBox::BorderBox)
        | G::ElementDependent
        | G::FillBox
        | G::StrokeBox
        | G::ViewBox => ClipBox::Border,
    }
}

fn shape_radius(
    epoch: u64,
    r: &style::values::generics::basic_shape::GenericShapeRadius<
        style::values::computed::LengthPercentage,
    >,
) -> ShapeRadius {
    use style::values::generics::basic_shape::GenericShapeRadius as R;
    match r {
        R::Length(l) => ShapeRadius::Length(lp(epoch, &l.0)),
        R::ClosestSide => ShapeRadius::ClosestSide,
        R::FarthestSide => ShapeRadius::FarthestSide,
        R::ClosestCorner => ShapeRadius::ClosestCorner,
        R::FarthestCorner => ShapeRadius::FarthestCorner,
    }
}

fn shape_center(
    epoch: u64,
    p: &style::values::generics::position::GenericPositionOrAuto<style::values::computed::Position>,
) -> (LengthPercentage, LengthPercentage) {
    match p {
        style::values::generics::position::GenericPositionOrAuto::Position(p) => {
            (lp(epoch, &p.horizontal), lp(epoch, &p.vertical))
        }
        style::values::generics::position::GenericPositionOrAuto::Auto => (
            LengthPercentage::Percent(0.5),
            LengthPercentage::Percent(0.5),
        ),
    }
}

/// SVG endpoint arc to cubic Beziers (SVG 1.1 implementation notes F.6, curves of at most 90 degrees).
#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    from: (f32, f32),
    to: (f32, f32),
    rx: f32,
    ry: f32,
    rotate_deg: f32,
    large: bool,
    sweep: bool,
    out: &mut Vec<PathSegment>,
) {
    let (mut rx, mut ry) = (f64::from(rx.abs()), f64::from(ry.abs()));
    let (x1, y1, x2, y2) = (
        f64::from(from.0),
        f64::from(from.1),
        f64::from(to.0),
        f64::from(to.1),
    );
    if (x1 == x2 && y1 == y2) || rx == 0.0 || ry == 0.0 {
        out.push(PathSegment::Line(to.0, to.1));
        return;
    }
    let phi = f64::from(rotate_deg).to_radians();
    let (sp, cp) = phi.sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (x1p, y1p) = (cp * dx + sp * dy, -sp * dx + cp * dy);
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den == 0.0 {
        0.0
    } else {
        (num / den).max(0.0).sqrt()
    };
    if large == sweep {
        coef = -coef;
    }
    let (cxp, cyp) = (coef * rx * y1p / ry, -coef * ry * x1p / rx);
    let (cx, cy) = (
        cp * cxp - sp * cyp + (x1 + x2) / 2.0,
        sp * cxp + cp * cyp + (y1 + y2) / 2.0,
    );
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = angle(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }
    let n = (dtheta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = dtheta / n as f64;
    let t = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |a: f64| {
        let (sa, ca) = a.sin_cos();
        (
            cx + rx * ca * cp - ry * sa * sp,
            cy + rx * ca * sp + ry * sa * cp,
        )
    };
    let deriv = |a: f64| {
        let (sa, ca) = a.sin_cos();
        (-rx * sa * cp - ry * ca * sp, -rx * sa * sp + ry * ca * cp)
    };
    for i in 0..n {
        let (a0, a1) = (theta1 + step * i as f64, theta1 + step * (i + 1) as f64);
        let (p0, p3) = (point(a0), point(a1));
        let (d0, d1) = (deriv(a0), deriv(a1));
        let end = if i + 1 == n {
            (f64::from(to.0), f64::from(to.1))
        } else {
            p3
        };
        out.push(PathSegment::Cubic(
            ((p0.0 + t * d0.0) as f32, (p0.1 + t * d0.1) as f32),
            ((p3.0 - t * d1.0) as f32, (p3.1 - t * d1.1) as f32),
            (end.0 as f32, end.1 as f32),
        ));
    }
}

/// The segments of a `path()` as absolute px: relative commands made absolute, `H`/`V`/`T`/`S`/`Q`
/// reduced, arcs turned into cubics.
fn path_segments(data: &style::values::specified::svg_path::SVGPathData) -> Vec<PathSegment> {
    use style::values::generics::basic_shape::{ArcSize, ArcSweep, GenericShapeCommand as C};
    use style::values::specified::svg_path::CoordPair;
    let norm = data.normalize(true);
    let mut out = Vec::new();
    let mut pos = (0.0f32, 0.0f32);
    let mut start = pos;
    for c in norm.commands() {
        match c {
            C::Close => {
                out.push(PathSegment::Close);
                pos = start;
            }
            C::Move { point } => {
                let p = CoordPair::from(*point);
                out.push(PathSegment::Move(p.x, p.y));
                pos = (p.x, p.y);
                start = pos;
            }
            C::Line { point } => {
                let p = CoordPair::from(*point);
                out.push(PathSegment::Line(p.x, p.y));
                pos = (p.x, p.y);
            }
            C::CubicCurve {
                point,
                control1,
                control2,
            } => {
                let (p, a, b) = (
                    CoordPair::from(*point),
                    CoordPair::from(*control1),
                    CoordPair::from(*control2),
                );
                out.push(PathSegment::Cubic((a.x, a.y), (b.x, b.y), (p.x, p.y)));
                pos = (p.x, p.y);
            }
            C::Arc {
                point,
                radii,
                arc_sweep,
                arc_size,
                rotate,
            } => {
                let p = CoordPair::from(*point);
                let ry = match &radii.ry {
                    style::values::generics::Optional::Some(v) => *v,
                    style::values::generics::Optional::None => radii.rx,
                };
                arc_to_cubics(
                    pos,
                    (p.x, p.y),
                    radii.rx,
                    ry,
                    *rotate,
                    matches!(arc_size, ArcSize::Large),
                    matches!(arc_sweep, ArcSweep::Cw),
                    &mut out,
                );
                pos = (p.x, p.y);
            }
            // `reduce` leaves only M, L, C, A and Z.
            C::HLine { .. }
            | C::VLine { .. }
            | C::QuadCurve { .. }
            | C::SmoothCubic { .. }
            | C::SmoothQuad { .. } => {}
        }
    }
    out
}

pub(crate) fn clip_path(cv: &ComputedValues, epoch: u64) -> ClipPath {
    use style::values::generics::basic_shape::{
        FillRule, GenericBasicShape as S, GenericClipPath as P, GenericPathOrShapeFunction as F,
    };
    match &cv.get_svg().clip_path {
        P::None => ClipPath::None,
        P::Url(_) => ClipPath::Url,
        P::Box(b) => ClipPath::Box(clip_box(b)),
        P::Shape(shape, b) => {
            let reference = clip_box(b);
            let shape = match &**shape {
                S::Rect(r) => {
                    let rr = &r.round;
                    let corner = |c: &style::values::computed::BorderCornerRadius| {
                        (lp(epoch, &c.0.width.0), lp(epoch, &c.0.height.0))
                    };
                    BasicShape::Inset {
                        insets: [
                            lp(epoch, &r.rect.0),
                            lp(epoch, &r.rect.1),
                            lp(epoch, &r.rect.2),
                            lp(epoch, &r.rect.3),
                        ],
                        radii: [
                            corner(&rr.top_left),
                            corner(&rr.top_right),
                            corner(&rr.bottom_right),
                            corner(&rr.bottom_left),
                        ],
                    }
                }
                S::Circle(c) => BasicShape::Circle {
                    radius: shape_radius(epoch, &c.radius),
                    center: shape_center(epoch, &c.position),
                },
                S::Ellipse(e) => BasicShape::Ellipse {
                    rx: shape_radius(epoch, &e.semiaxis_x),
                    ry: shape_radius(epoch, &e.semiaxis_y),
                    center: shape_center(epoch, &e.position),
                },
                S::Polygon(p) => BasicShape::Polygon {
                    evenodd: matches!(p.fill, FillRule::Evenodd),
                    points: p
                        .coordinates
                        .iter()
                        .map(|c| (lp(epoch, &c.0), lp(epoch, &c.1)))
                        .collect(),
                },
                S::PathOrShape(F::Path(p)) => BasicShape::Path {
                    evenodd: matches!(p.fill, FillRule::Evenodd),
                    segments: path_segments(&p.path),
                },
                S::PathOrShape(F::Shape(_)) => {
                    return ClipPath::Unsupported("css.value.shape-function");
                }
            };
            ClipPath::Shape {
                shape: Box::new(shape),
                reference,
            }
        }
    }
}
