//! Procedural backdrop textures, built once as RGBA pixels (no encoding round trip).
use iced::widget::image::Handle;

/// Faint dot grid, faded toward the edges. Drawn at very low alpha.
pub fn dots() -> Handle {
    const W: u32 = 1920;
    const H: u32 = 1080;
    const STEP: u32 = 24;
    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in (STEP / 2..H).step_by(STEP as usize) {
        for x in (STEP / 2..W).step_by(STEP as usize) {
            let nx = (x as f32 / W as f32 - 0.5) * 2.0;
            let ny = (y as f32 / H as f32 - 0.35) * 2.0;
            let fade = (1.0 - (nx * nx * 0.6 + ny * ny * 0.9).sqrt()).clamp(0.0, 1.0);
            let a = (fade * 255.0) as u8;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = (((y + dy) * W + x + dx) * 4) as usize;
                buf[i..i + 4].copy_from_slice(&[
                    crate::theme::DOT[0],
                    crate::theme::DOT[1],
                    crate::theme::DOT[2],
                    a,
                ]);
            }
        }
    }
    Handle::from_rgba(W, H, buf)
}

/// Barely-there radial blue glow (smooth, so a low resolution scales cleanly).
pub fn glow() -> Handle {
    const W: u32 = 512;
    const H: u32 = 256;
    let mut buf = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let dx = (x as f32 / W as f32 - 0.78) * 1.6;
            let dy = y as f32 / H as f32;
            let r = (dx * dx + dy * dy).sqrt().min(1.0);
            let fade_y = (1.0 - y as f32 / H as f32).powi(2);
            let a = ((1.0 - r).powi(2) * 0.22 * fade_y * 255.0) as u8;
            buf.extend_from_slice(&[
                crate::theme::BACKDROP_GLOW[0],
                crate::theme::BACKDROP_GLOW[1],
                crate::theme::BACKDROP_GLOW[2],
                a,
            ]);
        }
    }
    Handle::from_rgba(W, H, buf)
}
