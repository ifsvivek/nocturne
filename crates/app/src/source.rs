//! `nocturne_core::source` plus the GPUI mapping of its image sniffer.
pub use nocturne_core::source::*;

/// Like `nocturne_core::source::sniff`, but yields GPUI's format type.
pub fn sniff(b: &[u8]) -> Option<gpui::ImageFormat> {
    use gpui::ImageFormat as G;
    use nocturne_core::source::{ImgFormat, sniff};
    sniff(b).map(|f| match f {
        ImgFormat::Png => G::Png,
        ImgFormat::Jpeg => G::Jpeg,
        ImgFormat::Webp => G::Webp,
        ImgFormat::Gif => G::Gif,
    })
}
