use windows::Win32::Foundation::COLORREF;

/// Couleur du texte (gris tres fonce, plus doux que le noir pur).
pub const TEXT_COLOR: COLORREF = rgb(43, 43, 43);
pub const MUTED_COLOR: COLORREF = rgb(105, 100, 88);
pub const ACCENT_COLOR: COLORREF = rgb(88, 82, 70);

pub const DEFAULT_W: i32 = 260;
pub const DEFAULT_H: i32 = 220;

/// Six teintes de papier.
pub const PALETTE: [COLORREF; 6] = [
    rgb(255, 243, 163), // jaune
    rgb(255, 209, 220), // rose
    rgb(207, 245, 193), // vert
    rgb(205, 231, 255), // bleu
    rgb(230, 212, 255), // violet
    rgb(255, 217, 179), // orange
];

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// Assombrit une couleur de `pct` pour cent.
pub fn darken(color: COLORREF, pct: u32) -> COLORREF {
    let keep = 100u32.saturating_sub(pct);
    let r = ((color.0 & 0xFF) * keep / 100) as u8;
    let g = (((color.0 >> 8) & 0xFF) * keep / 100) as u8;
    let b = (((color.0 >> 16) & 0xFF) * keep / 100) as u8;
    rgb(r, g, b)
}

pub fn paper(index: u8) -> COLORREF {
    PALETTE[index as usize % PALETTE.len()]
}
