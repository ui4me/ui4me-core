//! Small, bounded PNG application icons. Decode once during host startup,
//! before the GUI runs; no filesystem work is done during a window callback.
use anyhow::{bail, Context, Result};
use std::io::{Cursor, Read};
use std::path::Path;
use winit::icon::{Icon, RgbaIcon};

const SOURCE_LIMIT: usize = 4 * 1024 * 1024;
const DECODE_LIMIT: usize = 16 * 1024 * 1024;
const DEFAULT_ICON: &[u8] = include_bytes!("../assets/ui4me.png");

pub fn load(path: Option<&Path>) -> Result<Icon> {
    if let Some(path) = path {
        let file = std::fs::File::open(path)
            .with_context(|| format!("Open application PNG icon {}", path.display()))?;
        let mut data = Vec::new();
        file.take((SOURCE_LIMIT + 1) as u64)
            .read_to_end(&mut data)?;
        if data.len() > SOURCE_LIMIT {
            bail!("Application PNG icon exceeds 4 MiB");
        }
        decode(&data)
    } else {
        decode(DEFAULT_ICON)
    }
}

fn decode(data: &[u8]) -> Result<Icon> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_limits(png::Limits {
        bytes: DECODE_LIMIT,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .context("Application icon must be a valid PNG")?;
    let header = reader.info();
    if header.width == 0 || header.height == 0 || header.width > 1024 || header.height > 1024 {
        bail!("Application icon dimensions must be between 1 and 1024 pixels");
    }
    let length = reader
        .output_buffer_size()
        .context("PNG decoded size overflows")?;
    if length > DECODE_LIMIT {
        bail!("PNG decoded size exceeds its memory limit");
    }
    let mut decoded = vec![0; length];
    let info = reader.next_frame(&mut decoded)?;
    let bytes = &decoded[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => bail!("PNG palette expansion was unavailable"),
    };
    Ok(RgbaIcon::new(rgba, info.width, info.height)?.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_icon_decodes_and_invalid_assets_fail_before_native_window_creation() {
        assert!(load(None).is_ok());
        assert!(decode(b"not a PNG").is_err());
        assert!(decode(&DEFAULT_ICON[..DEFAULT_ICON.len() / 2]).is_err());
    }
}
