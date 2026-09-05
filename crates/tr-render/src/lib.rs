//! Native viewport presentation. CPU produces the opaque sRGB buffer.
//! Nearest presentation avoids introducing an undeclared gamma-domain filter.
pub mod diagnostic;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use tr_core::{
    ViewTransform,
    color::{LinearImage, Pixel, display_pixel},
};

pub struct PreparedImage {
    pub rgba: Vec<u8>,
    pub histogram: [[u32; 256]; 3],
}
pub fn prepare(image: &LinearImage) -> PreparedImage {
    PreparedImage {
        rgba: image.to_display(),
        histogram: image.histogram(),
    }
}
pub struct Sample {
    pub x: u32,
    pub y: u32,
    pub working: Pixel,
    pub display: [u8; 4],
}
pub fn viewport(
    ui: &mut egui::Ui,
    texture: &TextureHandle,
    raster: &LinearImage,
    transform: &mut ViewTransform,
    id: impl std::hash::Hash + std::fmt::Debug,
) -> (egui::Response, Option<Sample>) {
    let available = ui.available_size().max(Vec2::splat(20.0));
    let (rect, _) = ui.allocate_exact_size(available, Sense::hover());
    let response = ui.interact(rect, ui.id().with(id), Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let scale = transform.scale(
        [raster.width as f32, raster.height as f32],
        [rect.width(), rect.height()],
        ppp,
    );
    if response.dragged() {
        let delta = ui.input(|i| i.pointer.delta());
        transform.center[0] =
            (transform.center[0] - delta.x / (raster.width as f32 * scale)).clamp(0.0, 1.0);
        transform.center[1] =
            (transform.center[1] - delta.y / (raster.height as f32 * scale)).clamp(0.0, 1.0);
    }
    if response.hovered() {
        let delta = ui.input(|i| i.smooth_scroll_delta.y);
        if delta.abs() > 0.01 {
            let old_zoom = transform.zoom.unwrap_or(scale * ppp);
            transform.set_zoom(old_zoom * (delta * 0.002).exp());
        }
    }
    let scale = transform.scale(
        [raster.width as f32, raster.height as f32],
        [rect.width(), rect.height()],
        ppp,
    );
    let size = Vec2::new(raster.width as f32 * scale, raster.height as f32 * scale);
    let top = rect.center() - Vec2::new(size.x * transform.center[0], size.y * transform.center[1]);
    let top = Pos2::new((top.x * ppp).round() / ppp, (top.y * ppp).round() / ppp);
    let image_rect = Rect::from_min_size(top, size);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0, Color32::from_gray(119));
    painter.image(
        texture.id(),
        image_rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
        Color32::WHITE,
    );
    let sample = response
        .hover_pos()
        .filter(|p| image_rect.contains(*p))
        .map(|p| {
            let x = (((p.x - top.x) / scale).floor() as u32).min(raster.width - 1);
            let y = (((p.y - top.y) / scale).floor() as u32).min(raster.height - 1);
            let working = raster.pixels[(y * raster.width + x) as usize];
            Sample {
                x,
                y,
                working,
                display: display_pixel(working, 119. / 255.),
            }
        });
    (response, sample)
}
pub fn histogram(ui: &mut egui::Ui, bins: &[[u32; 256]; 3]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 70.), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3, Color32::from_gray(20));
    let max = bins.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
    for f in [0.25, 0.5, 0.75] {
        let x = rect.left() + rect.width() * f;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1., Color32::from_gray(42)),
        );
    }
    for (channel, color) in [
        Color32::from_rgb(200, 110, 100),
        Color32::from_rgb(121, 173, 131),
        Color32::from_rgb(115, 151, 194),
    ]
    .into_iter()
    .enumerate()
    {
        let points = bins[channel]
            .iter()
            .enumerate()
            .map(|(i, v)| {
                Pos2::new(
                    rect.left() + i as f32 / 255. * rect.width(),
                    rect.bottom() - ((*v as f32 + 1.).ln() / (max + 1.).ln()) * rect.height(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1., color)));
    }
}
