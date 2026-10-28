//! Viewport sampling uses the native composite, including physical-pixel 1:1.
//! Only the grid derivative is capped; the document and viewer retain all pixels.
use super::*;
use tr_core::resample::{Pyramid, Region};
impl Editor {
    pub(super) fn render_texture(&mut self, ctx: &egui::Context) {
        if !self.needs_render || self.doc.is_none() || !self.admit_edit(0) {
            return;
        }
        self.effects.invalidate_document_input();
        let result = (|| -> Result<()> {
            let doc = self.doc.as_ref().unwrap();
            let composite = Pyramid::new(doc.render()?)?;
            let factor = (2048. / doc.width.max(doc.height) as f64).min(1.);
            let size = [
                (doc.width as f64 * factor).round().max(1.) as u32,
                (doc.height as f64 * factor).round().max(1.) as u32,
            ];
            let region = Region::fitted([doc.width, doc.height], size);
            let image = composite.render(region)?;
            self.texture = Some(ctx.load_texture(
                "document-thumbnail",
                display(doc, &image, region),
                egui::TextureOptions::LINEAR,
            ));
            self.composite = Some(composite);
            self.view_region = None;
            Ok(())
        })();
        if let Err(e) = result {
            self.error = e.to_string();
        }
        self.needs_render = false;
    }
    pub(super) fn paint_composite(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        viewport: egui::Rect,
        scale: f32,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let visible = rect.intersect(viewport);
        if !visible.is_positive() {
            return;
        }
        let size = [
            (visible.width() * ppp).round().max(1.) as u32,
            (visible.height() * ppp).round().max(1.) as u32,
        ];
        let region = Region {
            size,
            origin: [
                ((visible.left() - rect.left()) / scale) as f64,
                ((visible.top() - rect.top()) / scale) as f64,
            ],
            step: [1. / (scale * ppp) as f64; 2],
        };
        if self.view_region != Some(region) {
            let result = self.composite.as_ref().unwrap().render(region);
            match result {
                Ok(image) => {
                    self.view_texture = Some(ui.ctx().load_texture(
                        "document-viewport",
                        display(self.doc.as_ref().unwrap(), &image, region),
                        egui::TextureOptions::NEAREST,
                    ));
                    self.view_region = Some(region);
                }
                Err(e) => {
                    self.error = e.to_string();
                    return;
                }
            }
        }
        if let Some(texture) = &self.view_texture {
            // The returned buffer maps one-to-one onto backing pixels.
            let painted = egui::Rect::from_min_size(
                visible.min,
                egui::vec2(size[0] as f32, size[1] as f32) / ppp,
            );
            ui.painter().image(
                texture.id(),
                painted,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
    }
}
fn display(doc: &Document, image: &LinearImage, region: Region) -> egui::ColorImage {
    let size = [image.width as usize, image.height as usize];
    let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
    for (i, pixel) in image.pixels.iter().enumerate() {
        let x = i % size[0];
        let y = i / size[0];
        let background = if ((x / 12) + (y / 12)).is_multiple_of(2) {
            0.16
        } else {
            0.24
        };
        let mut rgba = tr_core::color::display_pixel(*pixel, background);
        if !doc.selection.is_empty() {
            let sx = (region.origin[0] + (x as f64 + 0.5) * region.step[0]).floor() as i64;
            let sy = (region.origin[1] + (y as f64 + 0.5) * region.step[1]).floor() as i64;
            if sx >= 0 && sy >= 0 && sx < doc.width as i64 && sy < doc.height as i64 {
                let (sx, sy) = (sx as usize, sy as usize);
                let width = doc.width as usize;
                let selected = doc.selection[sy * width + sx] >= 0.5;
                let left = doc.selection[sy * width + sx.saturating_sub(1)] >= 0.5;
                let above = doc.selection[sy.saturating_sub(1) * width + sx] >= 0.5;
                if selected != left || selected != above || (selected && (sx == 0 || sy == 0)) {
                    rgba = [255, 210, 0, 255];
                }
            }
        }
        bytes.extend_from_slice(&rgba);
    }
    egui::ColorImage::from_rgba_unmultiplied(size, &bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_viewport_above_2048_preserves_single_pixels_at_one_to_one() {
        let mut e = Editor::new(
            mpsc::sync_channel(1).0,
            tr_core::budget::MemoryBudget::new(4 * 1024 * 1024 * 1024),
        );
        let mut doc = Document::new(4096, 2, 240., [0., 0., 0., 1.]).unwrap();
        doc.layers[0].pixels[3000] = [0.2, 0.4, 0.6, 1.];
        e.replace(doc.clone());
        e.render_texture(&egui::Context::default());
        let region = Region {
            size: [2, 1],
            origin: [3000., 0.],
            step: [1., 1.],
        };
        let image = e.composite.as_ref().unwrap().render(region).unwrap();
        assert_eq!(image.pixels, &doc.layers[0].pixels[3000..3002]);
        assert_eq!(e.composite.as_ref().unwrap().source().width, 4096);
        assert_eq!(e.texture.as_ref().unwrap().size()[0], 2048);
    }
}
