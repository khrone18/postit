use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let icon_path = out_dir.join("postit.ico");
    if let Err(err) = write_icon(&icon_path) {
        println!("cargo:warning=icone non generee: {err}");
    }

    let mut res = winresource::WindowsResource::new();
    if icon_path.exists() {
        if let Some(path) = icon_path.to_str() {
            res.set_icon(path);
        }
    }
    res.set("ProductName", "Post-it");
    res.set("FileDescription", "Post-it pour le bureau Windows");
    res.set("OriginalFilename", "postit.exe");
    res.set("LegalCopyright", "MIT");
    if let Err(err) = res.compile() {
        println!("cargo:warning=ressources Windows non compilees: {err}");
    }
}

/// Genere une icone multi-tailles (16 a 256 px) sans ressource binaire externe.
fn write_icon(path: &Path) -> io::Result<()> {
    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&size| render(size)).collect();

    let mut file = BufWriter::new(File::create(path)?);
    file.write_all(&0u16.to_le_bytes())?; // reserved
    file.write_all(&1u16.to_le_bytes())?; // type: icone
    file.write_all(&(SIZES.len() as u16).to_le_bytes())?;

    let mut offset = 6 + 16 * SIZES.len();
    for (size, data) in SIZES.iter().zip(images.iter()) {
        let dimension = if *size >= 256 { 0 } else { *size as u8 };
        file.write_all(&[dimension, dimension, 0, 0])?;
        file.write_all(&1u16.to_le_bytes())?; // plans
        file.write_all(&32u16.to_le_bytes())?; // bits par pixel
        file.write_all(&(data.len() as u32).to_le_bytes())?;
        file.write_all(&(offset as u32).to_le_bytes())?;
        offset += data.len();
    }
    for data in &images {
        file.write_all(data)?;
    }
    file.flush()
}

fn render(size: u32) -> Vec<u8> {
    let width = size as usize;
    let height = size as usize;
    let scale = size as f32;

    let margin = (scale * 0.09).max(1.0);
    let left = margin;
    let top = margin;
    let right = scale - margin;
    let bottom = scale - margin * 1.35;
    let radius = scale * 0.15;
    let fold = scale * 0.34;

    let mut pixels = vec![[0u8; 4]; width * height];

    for y in 0..height {
        for x in 0..width {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            if !rounded_rect(px, py, left, top, right, bottom, radius) {
                continue;
            }

            let mut color = [255u8, 222, 89, 255]; // jaune

            // Coin plie en bas a droite.
            let fold_x = right - fold;
            let fold_y = bottom - fold;
            if px > fold_x && py > fold_y {
                if (px - fold_x) + (bottom - py) < fold {
                    color = [235, 190, 55, 255];
                }
            }

            // Trois lignes de texte suggerees.
            if size >= 24 && px > left + scale * 0.18 && px < right - scale * 0.30 {
                let line_height = (scale * 0.045).max(1.0);
                let spacing = scale * 0.16;
                for line in 0..3 {
                    let ly = top + scale * 0.31 + line as f32 * spacing;
                    if (py - ly).abs() <= line_height {
                        color = [130, 102, 22, 245];
                    }
                }
            }

            pixels[y * width + x] = color;
        }
    }

    // BITMAPINFOHEADER + pixels BGRA (bas en haut) + masque AND.
    let mask_row = ((width + 31) / 32) * 4;
    let mut out = Vec::with_capacity(40 + width * height * 4 + mask_row * height);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&((height * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((width * height * 4) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);

    for y in (0..height).rev() {
        for x in 0..width {
            let [r, g, b, a] = pixels[y * width + x];
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, mask_row * height));
    out
}

fn rounded_rect(px: f32, py: f32, left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> bool {
    if px < left || px > right || py < top || py > bottom {
        return false;
    }
    let cx = px.clamp(left + radius, right - radius);
    let cy = py.clamp(top + radius, bottom - radius);
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= radius * radius
}
