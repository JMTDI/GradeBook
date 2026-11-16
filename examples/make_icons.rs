//! Regenerates the desktop app icons from `assets/AppIcon.png`:
//!
//!     cargo run --example make_icons
//!
//! Writes rounded-corner PNGs at the sizes the window, Linux packages and the macOS
//! `.icns` need, plus a multi-size Windows `.ico`, into `assets/icons/`.

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

/// Premultiplied RGBA, 0.0..=1.0 per channel.
struct Image {
    size: usize,
    px: Vec<[f32; 4]>,
}

fn load(path: &Path) -> Image {
    let mut reader = png::Decoder::new(BufReader::new(File::open(path).expect("open source icon")))
        .read_info()
        .expect("read png header");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut buf).expect("decode png");
    assert_eq!(info.width, info.height, "icon must be square");
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => panic!("unsupported colour type {other:?}"),
    };
    let size = info.width as usize;
    let px = buf[..size * size * channels]
        .chunks(channels)
        .map(|p| {
            let a = if channels == 4 { p[3] as f32 / 255.0 } else { 1.0 };
            [
                p[0] as f32 / 255.0 * a,
                p[1] as f32 / 255.0 * a,
                p[2] as f32 / 255.0 * a,
                a,
            ]
        })
        .collect();
    Image { size, px }
}

/// Clips the image to a rounded square with anti-aliased edges.
fn round_corners(img: &mut Image, radius: f32) {
    let half = img.size as f32 / 2.0;
    for y in 0..img.size {
        for x in 0..img.size {
            let (dx, dy) = (
                (x as f32 + 0.5 - half).abs() - (half - radius),
                (y as f32 + 0.5 - half).abs() - (half - radius),
            );
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - radius;
            let coverage = (0.5 - outside).clamp(0.0, 1.0);
            for c in &mut img.px[y * img.size + x] {
                *c *= coverage;
            }
        }
    }
}

/// Area-averaging downscale (exact box filter for any ratio).
fn downscale(img: &Image, size: usize) -> Image {
    if size == img.size {
        return Image {
            size,
            px: img.px.clone(),
        };
    }
    let scale = img.size as f32 / size as f32;
    let mut px = vec![[0.0f32; 4]; size * size];
    for y in 0..size {
        for x in 0..size {
            let (x0, x1, y0, y1) = (
                x as f32 * scale,
                (x + 1) as f32 * scale,
                y as f32 * scale,
                (y + 1) as f32 * scale,
            );
            let mut acc = [0.0f32; 4];
            let mut total = 0.0f32;
            for sy in (y0.floor() as usize)..(y1.ceil() as usize).min(img.size) {
                let wy = ((sy + 1) as f32).min(y1) - (sy as f32).max(y0);
                for sx in (x0.floor() as usize)..(x1.ceil() as usize).min(img.size) {
                    let w = wy * (((sx + 1) as f32).min(x1) - (sx as f32).max(x0));
                    let p = img.px[sy * img.size + sx];
                    for c in 0..4 {
                        acc[c] += p[c] * w;
                    }
                    total += w;
                }
            }
            px[y * size + x] = acc.map(|v| v / total);
        }
    }
    Image { size, px }
}

/// Places `body` (already downscaled) in the middle of a transparent square canvas.
fn centred_in(body: &Image, canvas: usize) -> Image {
    let mut px = vec![[0.0f32; 4]; canvas * canvas];
    let offset = (canvas - body.size) / 2;
    for y in 0..body.size {
        for x in 0..body.size {
            px[(y + offset) * canvas + x + offset] = body.px[y * body.size + x];
        }
    }
    Image { size: canvas, px }
}

fn to_rgba8(img: &Image) -> Vec<u8> {
    img.px
        .iter()
        .flat_map(|p| {
            let a = p[3];
            let un = |c: f32| if a > 0.0 { (c / a).clamp(0.0, 1.0) } else { 0.0 };
            [un(p[0]), un(p[1]), un(p[2]), a].map(|v| (v * 255.0 + 0.5) as u8)
        })
        .collect()
}

fn encode_png(img: &Image) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, img.size as u32, img.size as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&to_rgba8(img))
        .unwrap();
    out
}

/// An `.ico` made of PNG-compressed images (supported since Windows Vista).
fn encode_ico(images: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, data) in images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}

/// An `.icns` made of PNG-compressed images. Each entry is `(type, png)`; the types
/// are Apple's: `icp4`/`icp5`/`icp6` = 16/32/64 px, `ic07`..`ic10` = 128..1024 px, and
/// `ic11`..`ic14` = the 16/32/128/256 px "@2x" images (32/64/256/512 px).
fn encode_icns(entries: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let total = 8 + entries.iter().map(|(_, data)| 8 + data.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&(total as u32).to_be_bytes());
    for (kind, data) in entries {
        out.extend_from_slice(*kind);
        out.extend_from_slice(&((8 + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(data);
    }
    out
}

/// A macOS-style icon: the artwork sits inside a transparent margin (824 of 1024 points).
fn macos_icon(master: &Image, size: usize) -> Image {
    let margin = (size as f32 * (1.0 - 824.0 / 1024.0) / 2.0).round() as usize;
    centred_in(&downscale(master, size - 2 * margin), size)
}

fn write(path: &Path, bytes: &[u8]) {
    BufWriter::new(File::create(path).expect("create output"))
        .write_all(bytes)
        .expect("write output");
    println!("wrote {} ({} KB)", path.display(), bytes.len().div_ceil(1024));
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let out = root.join("icons");
    std::fs::create_dir_all(&out).unwrap();

    let mut master = load(&root.join("AppIcon.png"));
    // Roughly the corner radius of the macOS app icon shape.
    let radius = master.size as f32 * 0.2237;
    round_corners(&mut master, radius);

    let mut ico_images = Vec::new();
    for size in [16, 24, 32, 48, 64, 128, 256, 512] {
        let png = encode_png(&downscale(&master, size));
        if size <= 256 {
            ico_images.push((size, png.clone()));
        }
        write(&out.join(format!("icon-{size}.png")), &png);
    }
    write(&out.join("icon.ico"), &encode_ico(&ico_images));

    let mac = |size| encode_png(&macos_icon(&master, size));
    let icns = [
        (b"icp4", mac(16)),
        (b"icp5", mac(32)),
        (b"icp6", mac(64)),
        (b"ic07", mac(128)),
        (b"ic08", mac(256)),
        (b"ic09", mac(512)),
        (b"ic10", mac(1024)),
        (b"ic11", mac(32)),
        (b"ic12", mac(64)),
        (b"ic13", mac(256)),
        (b"ic14", mac(512)),
    ];
    write(&out.join("icon.icns"), &encode_icns(&icns));
}
