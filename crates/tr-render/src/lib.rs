//! Linear-light CPU filtering at the exact backing resolution; the GPU presents 1:1.
pub mod diagnostic;
pub mod presenter;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use presenter::Presenter;
use std::sync::Arc;
use tr_core::{
    ViewTransform,
    color::{LinearImage, Pixel, display_pixel},
    resample::{Pyramid, Region},
};

pub struct PreparedImage {
    pub pyramid: Arc<Pyramid>,
    pub histogram: [[u32; 256]; 3],
}
pub fn prepare(image: LinearImage) -> anyhow::Result<PreparedImage> {
    let histogram = image.histogram();
    Ok(PreparedImage {
        pyramid: Arc::new(Pyramid::new(image)?),
        histogram,
    })
}
pub struct Sample {
    pub x: u32,
    pub y: u32,
    pub working: Pixel,
    pub display: [u8; 4],
}
pub fn viewport(
    ui: &mut egui::Ui,
    presenter: &mut Presenter,
    image: &Arc<Pyramid>,
    transform: &mut ViewTransform,
    id: &str,
) -> (egui::Response, Option<Sample>) {
    let raster = image.source();
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
    let physical_size = Vec2::new(
        (raster.width as f32 * scale * ppp).round().max(1.),
        (raster.height as f32 * scale * ppp).round().max(1.),
    );
    let size = physical_size / ppp;
    let top = rect.center() - Vec2::new(size.x * transform.center[0], size.y * transform.center[1]);
    let top = Pos2::new((top.x * ppp).round() / ppp, (top.y * ppp).round() / ppp);
    let image_rect = Rect::from_min_size(top, size);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0, Color32::from_gray(119));
    let visible = image_rect.intersect(rect);
    let min = egui::pos2((visible.min.x * ppp).ceil(), (visible.min.y * ppp).ceil());
    let max = egui::pos2((visible.max.x * ppp).floor(), (visible.max.y * ppp).floor());
    if max.x > min.x && max.y > min.y {
        let region = Region {
            size: [(max.x - min.x) as u32, (max.y - min.y) as u32],
            origin: [
                ((min.x - top.x * ppp) as f64) * raster.width as f64 / physical_size.x as f64,
                ((min.y - top.y * ppp) as f64) * raster.height as f64 / physical_size.y as f64,
            ],
            step: [
                raster.width as f64 / physical_size.x as f64,
                raster.height as f64 / physical_size.y as f64,
            ],
        };
        presenter.paint(
            ui,
            id.to_owned(),
            image,
            Rect::from_min_max(min / ppp, max / ppp),
            region,
        );
    }
    let sample = response
        .hover_pos()
        .filter(|p| image_rect.contains(*p))
        .map(|p| {
            let x = (((p.x - top.x) / size.x * raster.width as f32).floor() as u32)
                .min(raster.width - 1);
            let y = (((p.y - top.y) / size.y * raster.height as f32).floor() as u32)
                .min(raster.height - 1);
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
