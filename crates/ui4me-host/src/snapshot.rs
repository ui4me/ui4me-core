//! CPU rendering diagnostics without a desktop, backend or application host.
//! This is a screen snapshot for qualification, not a printing/PDF subsystem.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufWriter, Read};
use std::path::Path;
use std::time::{Duration, Instant};

pub fn render(html: &Path, output: &Path, width: u32, height: u32, scale: f64) -> Result<Value> {
    if width == 0 || height == 0 || !scale.is_finite() || !(0.5..=4.0).contains(&scale) {
        bail!("width and height must be positive; scale must be finite and between 0.5 and 4");
    }
    let pixel_width = (f64::from(width) * scale).round();
    let pixel_height = (f64::from(height) * scale).round();
    if pixel_width * pixel_height > ui4me_renderer::MAX_PIXELS as f64 {
        bail!("Snapshot exceeds the renderer pixel budget");
    }
    let (pixel_width, pixel_height) = (pixel_width as u32, pixel_height as u32);
    let source = html.canonicalize().context("Resolve HTML source")?;
    let metadata = std::fs::metadata(&source).context("Inspect HTML source")?;
    if !metadata.is_file() {
        bail!("HTML source must be a regular file");
    }
    if metadata.len() > 1024 * 1024 {
        bail!("HTML source exceeds 1 MiB");
    }
    let mut options = File::options();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Reject a raced FIFO without blocking before the descriptor's type
        // can be checked. This flag does not change regular-file reads.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(&source).context("Open HTML source")?;
    if !file.metadata()?.is_file() {
        bail!("HTML source must be a regular file");
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        bail!("HTML source exceeds 1 MiB");
    }
    let html = std::str::from_utf8(&bytes).context("HTML must be UTF-8")?;
    let mut document =
        ui4me_renderer::Document::parse_hosted(html, source.parent(), pixel_width, pixel_height)?;
    document.set_viewport(pixel_width, pixel_height, scale)?;
    // Layout discovers images and font faces, so render once before deciding
    // whether resources are settled. Never convert a timeout into a pass.
    let deadline = Instant::now() + Duration::from_secs(15);
    let pixels = loop {
        document.poll_resources()?;
        let pixels = document.render(pixel_width, pixel_height, scale)?;
        if !document.has_pending_resources() {
            break pixels;
        }
        if Instant::now() >= deadline {
            bail!("Snapshot resources did not settle within 15 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).context("Create snapshot output directory")?;
    }
    let file = File::create(output).context("Create PNG")?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), pixel_width, pixel_height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    let rgb: Vec<u8> = pixels
        .iter()
        .flat_map(|pixel| [(*pixel >> 16) as u8, (*pixel >> 8) as u8, *pixel as u8])
        .collect();
    writer.write_image_data(&rgb)?;
    writer.finish()?;
    Ok(
        json!({"output":output,"source":source,"width":width,"height":height,"scale":scale,
        "pixel_width":pixel_width,"pixel_height":pixel_height,"pending_resources":false,
        "profile":ui4me_renderer::css_profile().0,"diagnostics":document.diagnostics()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_geometry_before_touching_files() {
        for (w, h, s) in [
            (0, 10, 1.0),
            (10, 0, 1.0),
            (10, 10, f64::NAN),
            (10, 10, 0.1),
            (10_000, 10_000, 4.0),
        ] {
            assert!(render(Path::new("missing"), Path::new("unused.png"), w, h, s).is_err());
        }
    }
    #[test]
    fn snapshot_uses_hosted_document_and_reports_physical_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("index.html");
        let target = dir.path().join("out.png");
        std::fs::write(
            &source,
            "<!doctype html><html><body style='margin:0;background:red'>Hi</body></html>",
        )
        .unwrap();
        let info = render(&source, &target, 32, 24, 2.0).unwrap();
        assert_eq!(info["pixel_width"], 64);
        assert_eq!(info["pixel_height"], 48);
        assert!(std::fs::metadata(&target).unwrap().len() > 40);
        std::fs::write(&source, "<div contenteditable>excluded</div>").unwrap();
        assert!(render(&source, &target, 32, 24, 1.0).is_err());
    }
}
