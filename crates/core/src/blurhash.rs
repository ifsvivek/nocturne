//! BlurHash decoder (https://blurha.sh): a ~30 character string becomes a soft colour
//! placeholder shown while the real image loads. Pure Rust, no dependencies.
use std::f32::consts::PI;

const B83: &[u8; 83] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%*+,-.:;=?@[]^_{|}~";

fn dec83(s: &[u8]) -> Option<u32> {
    s.iter().try_fold(0u32, |acc, c| {
        let d = B83.iter().position(|b| b == c)? as u32;
        Some(acc * 83 + d)
    })
}

fn to_linear(v: u32) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

fn sign_sq(v: f32) -> f32 {
    v * v.abs()
}

/// RGBA pixels (`w * h * 4`, opaque) for `hash`, or `None` if the hash is malformed.
pub fn decode(hash: &str, w: u32, h: u32) -> Option<Vec<u8>> {
    let b = hash.as_bytes();
    if b.len() < 6 || w == 0 || h == 0 {
        return None;
    }
    let flag = dec83(&b[..1])?;
    let (nx, ny) = ((flag % 9 + 1) as usize, (flag / 9 + 1) as usize);
    if b.len() != 4 + 2 * nx * ny {
        return None;
    }
    let max = (dec83(&b[1..2])? + 1) as f32 / 166.0;
    let dc = dec83(&b[2..6])?;
    let mut colors = vec![[
        to_linear(dc >> 16),
        to_linear((dc >> 8) & 255),
        to_linear(dc & 255),
    ]];
    for i in 1..nx * ny {
        let v = dec83(&b[4 + i * 2..6 + i * 2])?;
        let q = |n: u32| sign_sq((n as f32 - 9.0) / 9.0) * max;
        colors.push([q(v / 361), q(v / 19 % 19), q(v % 19)]);
    }
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut px = [0.0f32; 3];
            for j in 0..ny {
                for i in 0..nx {
                    let basis = (PI * x as f32 * i as f32 / w as f32).cos()
                        * (PI * y as f32 * j as f32 / h as f32).cos();
                    let c = colors[j * nx + i];
                    (0..3).for_each(|k| px[k] += c[k] * basis);
                }
            }
            out.extend([to_srgb(px[0]), to_srgb(px[1]), to_srgb(px[2]), 255]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc83(mut v: u32, len: usize) -> String {
        let mut s = vec![b'0'; len];
        for i in (0..len).rev() {
            s[i] = B83[(v % 83) as usize];
            v /= 83;
        }
        String::from_utf8(s).unwrap()
    }

    #[test]
    fn flat_colour_hash_decodes_to_that_colour() {
        // 1x1 components: size flag 0, max 0, then the DC colour (sRGB 0xFF8000).
        let h = format!("00{}", enc83(0xFF8000, 4));
        let px = decode(&h, 4, 3).unwrap();
        assert_eq!(px.len(), 4 * 3 * 4);
        for p in px.chunks(4) {
            assert_eq!(p, [255, 128, 0, 255]);
        }
    }

    #[test]
    fn real_hash_gives_varied_opaque_pixels() {
        let px = decode("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 32, 32).unwrap();
        assert_eq!(px.len(), 32 * 32 * 4);
        assert!(px.chunks(4).all(|p| p[3] == 255));
        assert_ne!(px[..4], px[px.len() - 4..], "corners differ");
    }

    #[test]
    fn rejects_malformed() {
        assert!(decode("", 4, 4).is_none());
        assert!(decode("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 0, 4).is_none());
        assert!(
            decode("LEHV6nWB2yk8pyo0adR*.7kCMdn", 4, 4).is_none(),
            "short"
        );
        assert!(
            decode("L€HV6nWB2yk8pyo0adR*.7kCMdnj", 4, 4).is_none(),
            "bad char"
        );
    }
}
