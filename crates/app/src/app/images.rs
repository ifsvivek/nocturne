//! Images for cells and the hero: blurhash stand-in first, then the bytes from the disk cache or
//! the source, decoded off the UI thread into a texture-ready `Handle::from_rgba` at the size it
//! is drawn. The view only reads (`pic`); `want` (called from `update`) starts what is missing.
use super::{App, Img, Message, RETRY_FAILED};
use crate::task::run;
use crate::widget::cell::Pic;
use iced::Task;
use iced::widget::image::Handle;
use nocturne_core::backend::Backend;
use nocturne_core::source::{Art, Media, image_key};
use std::time::Instant;
use tokio::sync::Semaphore;

/// Decodes at once (a fast scroll must not start a decode per cell).
static DECODE: Semaphore = Semaphore::const_new(3);

/// A wanted image: the item, which art, and the width it is drawn at (logical px).
pub type Want = (Media, Art, u32);

fn blur_key(hash: &str) -> String {
    format!("blur:{hash}")
}

/// Backdrops are big; they live in their own small cache so they cannot push posters out.
pub fn is_big(art: Art) -> bool {
    art == Art::Backdrop
}

/// Width requested from the source for a draw width at this scale; big images snap to steps so a
/// window resize does not refetch.
pub fn request_width(draw: u32, scale: f32, art: Art) -> u32 {
    let px = (draw as f32 * scale.max(1.0)).ceil() as u32;
    if is_big(art) {
        (px.div_ceil(320) * 320).clamp(640, 2560)
    } else {
        px.clamp(48, 1200)
    }
}

/// Decode `bytes` and shrink to `max_w` (local art can be far larger than the cell).
pub fn decode(bytes: &[u8], max_w: u32) -> Option<Handle> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width() > max_w && max_w > 0 {
        let h = (u64::from(img.height()) * u64::from(max_w) / u64::from(img.width())).max(1);
        img = img.resize_exact(max_w, h as u32, image::imageops::FilterType::Triangle);
    }
    let rgba = img.into_rgba8();
    Some(Handle::from_rgba(
        rgba.width(),
        rgba.height(),
        rgba.into_raw(),
    ))
}

/// Shrink `bytes` to `max_w` and decode; also the small JPEG / PNG to keep for next time.
fn shrink(bytes: &[u8], max_w: u32) -> Option<(Handle, Vec<u8>)> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width() > max_w && max_w > 0 {
        let h = (u64::from(img.height()) * u64::from(max_w) / u64::from(img.width())).max(1);
        img = img.resize_exact(max_w, h as u32, image::imageops::FilterType::Triangle);
    }
    let mut small = vec![];
    let (fmt, rgb) = if img.color().has_alpha() {
        (image::ImageFormat::Png, false)
    } else {
        (image::ImageFormat::Jpeg, true)
    };
    let enc = if rgb {
        image::DynamicImage::ImageRgb8(img.to_rgb8())
    } else {
        img.clone()
    };
    enc.write_to(&mut std::io::Cursor::new(&mut small), fmt)
        .ok()?;
    let rgba = img.into_rgba8();
    let h = Handle::from_rgba(rgba.width(), rgba.height(), rgba.into_raw());
    Some((h, small))
}

/// Local art is read in full from the user's folder, often far larger than a cell: keep a shrunken
/// copy in the disk cache (keyed by the file's path, size and mtime) so a later scroll decodes
/// only that.
async fn fetch_local(
    b: Backend,
    m: Media,
    art: Art,
    req_w: u32,
    disk: Option<std::sync::Arc<nocturne_core::imgcache::DiskCache>>,
) -> Option<Handle> {
    let (Backend::Local(l), Some(d)) = (&b, disk) else {
        return fetch(b, m, art, req_w).await;
    };
    let key = format!("local|{}|{req_w}", l.image_sig(&m, art).await?);
    let _permit = DECODE.acquire().await.ok()?;
    let (l, d2) = (l.clone(), d.clone());
    if let Some(small) = tokio::task::spawn_blocking({
        let (d, key) = (d.clone(), key.clone());
        move || d.get(&key)
    })
    .await
    .ok()
    .flatten()
        && let Some(h) = tokio::task::spawn_blocking(move || decode(&small, req_w))
            .await
            .ok()
            .flatten()
    {
        return Some(h);
    }
    let bytes = l.image(&m, art).await?;
    tokio::task::spawn_blocking(move || {
        let (h, small) = shrink(&bytes, req_w)?;
        d2.put(&key, &small);
        Some(h)
    })
    .await
    .ok()
    .flatten()
}

async fn fetch(b: Backend, m: Media, art: Art, req_w: u32) -> Option<Handle> {
    let bytes = b.image(&m, art, req_w).await?;
    let _permit = DECODE.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || decode(&bytes, req_w))
        .await
        .ok()
        .flatten()
}

impl App {
    fn cache_for(&self, art: Art) -> &nocturne_core::cache::Cache<String, Img> {
        if is_big(art) {
            &self.heroes
        } else {
            &self.images
        }
    }

    /// What to draw for this image now. Never starts anything.
    pub(super) fn pic(&self, m: &Media, art: Art, draw_w: u32) -> Pic {
        let key = image_key(m, art, request_width(draw_w, self.scale, art), 0);
        match self.cache_for(art).get(&key) {
            Some(Img::Ready(h)) => return Pic::Ready(h.clone()),
            Some(Img::Failed(at)) if at.elapsed() < RETRY_FAILED => return Pic::Missing,
            _ => {}
        }
        match m.blur_for(art).and_then(|h| self.images.get(&blur_key(h))) {
            Some(Img::Ready(h)) => Pic::Blur(h.clone()),
            _ => Pic::Loading,
        }
    }

    /// Start whatever of `wants` is not cached or in flight. Called after a list or a scroll
    /// changes what is on screen.
    pub(super) fn want(&mut self, wants: &[Want]) -> Task<Message> {
        let mut tasks = vec![];
        for (m, art, draw_w) in wants {
            let art = *art;
            let req_w = request_width(*draw_w, self.scale, art);
            let key = image_key(m, art, req_w, 0);
            if let Some(h) = m.blur_for(art)
                && self.images.get(&blur_key(h)).is_none()
                && let Some(px) = nocturne_core::blurhash::decode(h, 32, 32)
            {
                self.images
                    .insert(blur_key(h), Img::Ready(Handle::from_rgba(32, 32, px)));
            }
            match self.cache_for(art).get(&key) {
                Some(Img::Ready(_) | Img::Loading) => continue,
                Some(Img::Failed(at)) if at.elapsed() < RETRY_FAILED => continue,
                _ => {}
            }
            let Some(b) = self.catalog.backend(m.origin).cloned() else {
                continue;
            };
            let big = is_big(art);
            let cache = if big {
                &mut self.heroes
            } else {
                &mut self.images
            };
            cache.insert(key.clone(), Img::Loading);
            let m = m.clone();
            let disk = self.disk.clone();
            tasks.push(run(fetch_local(b, m, art, req_w, disk), move |h| {
                Message::Image(key, big, h)
            }));
        }
        Task::batch(tasks)
    }

    pub(super) fn image_arrived(&mut self, key: String, big: bool, h: Option<Handle>) {
        let state = match h {
            Some(h) => Img::Ready(h),
            None => Img::Failed(Instant::now()),
        };
        if big {
            self.heroes.insert(key, state);
        } else {
            self.images.insert(key, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_widths_follow_scale_and_big_images_snap() {
        assert_eq!(request_width(176, 1.0, Art::Poster), 176);
        assert_eq!(request_width(176, 1.5, Art::Poster), 264);
        assert_eq!(request_width(1280, 1.0, Art::Backdrop), 1280);
        assert_eq!(request_width(1281, 1.0, Art::Backdrop), 1600);
        assert_eq!(request_width(3000, 2.0, Art::Backdrop), 2560);
    }

    #[test]
    fn decoding_shrinks_to_the_drawn_width() {
        let mut png = vec![];
        image::DynamicImage::new_rgba8(400, 600)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(decode(&png, 200).is_some());
        assert!(decode(b"not an image", 200).is_none());
    }
}
