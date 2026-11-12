//! owner: 008-layout-engine (L-INL-r2) — image boxes (FR-070–FR-073; T084, T085).
//!
//! `ImageInfo` is what box generation records about an `img` element: the alt text, the sizing
//! attributes (their ratio is used while the image is pending) and 007's natural-size view
//! (`ImageState`). `natural()` turns it into the replaced box's [`Natural`] geometry:
//!
//! * `Ready`: the density-corrected natural size and ratio from 007.
//! * `Pending` (or no resource information at all): natural size 0×0 with the attribute ratio,
//!   no `alt` shown, plus one `RESOURCE_PENDING` diagnostic (FR-072). After a resource-epoch
//!   advance the next layout uses the natural size.
//! * `Broken`: a sized image is an empty replaced box of the attribute size; an unsized image with
//!   non-empty `alt` becomes an inline text run (box generation); no broken-image icon (OD-37).

use ui4me_assets::view::ImageState;

use super::Natural;

#[derive(Clone, Debug, PartialEq)]
pub struct ImageInfo {
    pub alt: Option<String>,
    /// Pixel values of the `width` / `height` attributes, when both are present and positive.
    pub attr_size: Option<(f32, f32)>,
    /// `None` = the source carries no resource view: laid out as pending.
    pub state: Option<ImageState>,
}

impl ImageInfo {
    pub fn is_pending(&self) -> bool {
        matches!(self.state, None | Some(ImageState::Pending))
    }

    pub fn is_broken(&self) -> bool {
        matches!(self.state, Some(ImageState::Broken { .. }))
    }

    /// The natural geometry of the image box. `aspect_ratio` is the computed `aspect-ratio`
    /// (the `auto w / h` hint of the attributes), used only when 007 supplies no ratio.
    pub fn natural(&self, aspect_ratio: Option<f32>) -> Natural {
        let attr_ratio = self.attr_size.map(|(w, h)| w / h).or(aspect_ratio);
        match self.state {
            Some(ImageState::Ready {
                width_css,
                height_css,
                ratio,
            }) => {
                let ratio = ratio
                    .map(|(a, b)| a / b)
                    .filter(|r| r.is_finite() && *r > 0.0)
                    .or_else(|| match (width_css, height_css) {
                        (Some(w), Some(h)) if h > 0.0 => Some(w / h),
                        _ => None,
                    });
                Natural {
                    width: width_css,
                    height: height_css,
                    ratio: ratio.or(aspect_ratio),
                    ..Natural::default()
                }
            }
            Some(ImageState::Broken { .. }) => match self.attr_size {
                Some((w, h)) => Natural {
                    width: Some(w),
                    height: Some(h),
                    ratio: attr_ratio,
                    ..Natural::default()
                },
                None => Natural {
                    width: Some(0.0),
                    height: Some(0.0),
                    ratio: attr_ratio,
                    ..Natural::default()
                },
            },
            Some(ImageState::Pending) | None => Natural {
                width: Some(0.0),
                height: Some(0.0),
                ratio: attr_ratio,
                ..Natural::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::codes_generated::Code;

    #[test]
    fn ready_uses_the_natural_size_and_ratio() {
        let i = ImageInfo {
            alt: None,
            attr_size: None,
            state: Some(ImageState::Ready {
                width_css: Some(400.0),
                height_css: Some(300.0),
                ratio: None,
            }),
        };
        let n = i.natural(None);
        assert_eq!((n.width, n.height), (Some(400.0), Some(300.0)));
        assert!((n.ratio.unwrap() - 4.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn pending_is_zero_by_zero_with_the_attribute_ratio() {
        let i = ImageInfo {
            alt: Some("x".into()),
            attr_size: Some((200.0, 100.0)),
            state: Some(ImageState::Pending),
        };
        let n = i.natural(None);
        assert_eq!(
            (n.width, n.height, n.ratio),
            (Some(0.0), Some(0.0), Some(2.0))
        );
        assert!(i.is_pending());
        assert!(
            ImageInfo {
                alt: None,
                attr_size: None,
                state: None
            }
            .is_pending()
        );
    }

    #[test]
    fn broken_with_attributes_is_an_empty_box_of_that_size() {
        let i = ImageInfo {
            alt: None,
            attr_size: Some((30.0, 20.0)),
            state: Some(ImageState::Broken {
                code: Code::DecodeFailed,
            }),
        };
        let n = i.natural(None);
        assert_eq!((n.width, n.height), (Some(30.0), Some(20.0)));
        assert!(i.is_broken());
    }
}
