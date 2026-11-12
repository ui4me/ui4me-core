//! Public paged media (1.0 F1001): paginate the live document into pages (`@page` size and
//! margins, `print` media, page breaks, widows/orphans, repeated table headers, margin boxes),
//! report the pages and where elements fall, and render a page. The screen pipeline is never
//! changed; a pagination reflects the DOM and style when it is made and is reported stale after
//! a DOM change.

use super::*;
use crate::pipeline::paged::{PageOptions, PagedDocument};

/// Most pages one pagination may hold (bounded work and memory).
pub const MAX_PAGES: usize = 2000;
/// Most device pixels one rendered page may have (about 300 dpi on A3).
pub const MAX_PAGE_PIXELS: u64 = 5000 * 7100;

#[derive(Default)]
pub(super) struct PagedState {
    doc: Option<PagedDocument>,
}

/// A page size: a CSS page-size name (`A4`, `letter landscape`, ...) or CSS px.
#[derive(Clone, Debug)]
pub(crate) enum PageSize {
    Named(String),
    Px(f32, f32),
}

impl Document {
    /// Paginate with `size` (default: the document's `@page size`, else A4), `margins` (CSS px,
    /// top/right/bottom/left; default 48 = 0.5 in, `@page` margins win) and `print` media (default).
    pub(crate) fn paginate(
        &mut self,
        size: Option<PageSize>,
        margins: Option<[f32; 4]>,
        print: bool,
    ) -> Result<Value> {
        if self.hosted {
            return Err(arg(
                "Pagination and printing are outside the hosted desktop profile",
            ));
        }
        let (w, h) = match size {
            None => (PageOptions::A4.width, PageOptions::A4.height),
            Some(PageSize::Named(n)) => PageOptions::named(&n)
                .ok_or_else(|| arg(format!("Unknown page size {n:?} (A3, A4, A5, B4, B5, JIS-B4, JIS-B5, letter, legal, ledger, optionally landscape)")))?,
            Some(PageSize::Px(w, h)) => (w, h),
        };
        let opts = PageOptions {
            width: w,
            height: h,
            margin: margins.unwrap_or([48.0; 4]),
            print,
        };
        self.apply_follow()?;
        let paged = self.pipeline.paginate(&opts)?;
        if paged.page_count() > MAX_PAGES {
            return Err(limit_error(format!(
                "the document paginates into more than {MAX_PAGES} pages"
            )));
        }
        let pages: Vec<Value> = paged
            .pages
            .iter()
            .map(|p| {
                json!({"width": p.width, "height": p.height, "content": {
                    "x": p.content_origin().0, "y": p.content_origin().1,
                    "width": p.content_size().0, "height": p.content_size().1}})
            })
            .collect();
        let out =
            json!({"count": paged.page_count(), "pages": pages, "revision": paged.dom_revision});
        self.paged.doc = Some(paged);
        Ok(out)
    }
    fn current_pages(&self) -> Result<&PagedDocument> {
        if self.hosted {
            return Err(arg(
                "Pagination and printing are outside the hosted desktop profile",
            ));
        }
        let p = self
            .paged
            .doc
            .as_ref()
            .ok_or_else(|| arg("The document has not been paginated (call paginate first)"))?;
        if p.dom_revision != self.pipeline.document().revision() {
            return Err(Error {
                code: 4,
                message:
                    "The pagination is stale: the document changed since paginate; paginate again"
                        .into(),
            });
        }
        Ok(p)
    }
    /// Where `element` falls: `[{page, x, y, width, height}]` in page-box CSS px.
    pub(crate) fn page_rects(&self, element: &str) -> Result<Value> {
        let n = self.node(element)?;
        let p = self.current_pages()?;
        Ok(json!(p
            .element_rects(n)
            .into_iter()
            .map(|(page, r)| json!({"page": page, "x": r[0], "y": r[1], "width": r[2], "height": r[3]}))
            .collect::<Vec<_>>()))
    }
    /// The number of pages of the current pagination.
    pub(crate) fn page_count(&self) -> Result<usize> {
        Ok(self.current_pages()?.page_count())
    }
    /// Render page `index` at `dpi` (opaque, white page background; un-premultiplied RGBA).
    pub(crate) fn render_page(
        &self,
        index: usize,
        dpi: f32,
    ) -> Result<crate::raster::headless::PixelBuffer> {
        let p = self.current_pages()?;
        if let Some(page) = p.pages.get(index) {
            let scale = f64::from(dpi) / 96.0;
            let px =
                (f64::from(page.width) * scale).ceil() * (f64::from(page.height) * scale).ceil();
            if px > MAX_PAGE_PIXELS as f64 {
                return Err(limit_error(format!(
                    "page {index} at {dpi} dpi exceeds {MAX_PAGE_PIXELS} pixels"
                )));
            }
        }
        let mut b = p.render_page(index, dpi)?;
        if b.format == crate::raster::headless::PixelFormat::Rgba8Premultiplied {
            // The page is opaque (white under everything): premultiplied equals straight.
            b.format = crate::raster::headless::PixelFormat::Rgba8Unpremultiplied;
            for px in b.data.as_chunks_mut::<4>().0 {
                px[3] = 255;
            }
        }
        Ok(b)
    }
}
