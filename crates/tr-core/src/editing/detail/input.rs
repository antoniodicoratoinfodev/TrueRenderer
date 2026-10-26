//! Read-only pixel access lets global noise guides consume a point transform
//! directly, without allocating an intermediate full-resolution photograph.
use crate::{
    color::{LinearImage, Pixel},
    editing::regional::Rect,
};
use anyhow::{Result, ensure};

pub(in crate::editing) trait Input: Sync {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn pixel(&self, x: usize, y: usize) -> Pixel;
    fn cache_region(&self) -> bool {
        false
    }
}
pub(super) struct Window<'a, S: Input> {
    pub source: &'a S,
    pub rect: Rect,
    pub image: LinearImage,
}
impl<S: Input> Input for Window<'_, S> {
    fn width(&self) -> u32 {
        self.source.width()
    }
    fn height(&self) -> u32 {
        self.source.height()
    }
    #[inline]
    fn pixel(&self, x: usize, y: usize) -> Pixel {
        self.image.pixels[(y - self.rect.origin[1] as usize) * self.rect.size[0] as usize + x
            - self.rect.origin[0] as usize]
    }
}
impl Input for LinearImage {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    #[inline]
    fn pixel(&self, x: usize, y: usize) -> Pixel {
        self.pixels[y * self.width as usize + x]
    }
}
pub(super) fn extract(source: &impl Input, rect: Rect) -> Result<LinearImage> {
    ensure!(
        rect.size.into_iter().all(|v| v > 0)
            && (0..2).all(|a| u64::from(rect.origin[a]) + u64::from(rect.size[a])
                <= u64::from([source.width(), source.height()][a])),
        "Supporto rumore oltre sorgente"
    );
    let mut pixels = vec![[0.; 4]; rect.size[0] as usize * rect.size[1] as usize];
    crate::editing::pixels_mut(&mut pixels, |i, p| {
        *p = source.pixel(
            rect.origin[0] as usize + i % rect.size[0] as usize,
            rect.origin[1] as usize + i / rect.size[0] as usize,
        );
    });
    LinearImage::new(rect.size[0], rect.size[1], pixels)
}
