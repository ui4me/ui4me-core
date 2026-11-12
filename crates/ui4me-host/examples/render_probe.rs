//! Render the real probe to a PNG without a display server. This checks CPU
//! presentation only; it cannot qualify any native window/input behavior.
use std::io::BufWriter;
use ui4me_host::presentation::Probe;

fn main() -> anyhow::Result<()> {
    let output = std::env::args_os().nth(1).ok_or_else(|| {
        anyhow::anyhow!("usage: cargo run --example render_probe -- /path/to/probe.png")
    })?;
    let (width, height) = (900, 560);
    let mut pixels = vec![0_u32; (width * height) as usize];
    let mut probe = Probe::default();
    probe
        .set_text("Hello from UI4ME — type, select, copy and paste.")
        .map_err(anyhow::Error::msg)?;
    probe.focused = true;
    probe.select_bytes(11, 16);
    probe.status = "CPU snapshot: native operating-system behavior is not exercised.".into();
    probe.render(&mut pixels, width, height, 1.0, "UI4ME native host probe");
    let rgb: Vec<u8> = pixels
        .into_iter()
        .flat_map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
        .collect();
    let file = BufWriter::new(std::fs::File::create(output)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(())
}
