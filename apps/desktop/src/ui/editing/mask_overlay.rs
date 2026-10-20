//! Shared selection visualization. Interpolate coverage between vertices instead
//! of painting constant-alpha tiles. Work is bounded by the visible viewport,
//! including at high zoom; source geometry remains the caller's responsibility.
use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Shape};

pub(super) fn paint(
    ui: &egui::Ui,
    image: Rect,
    viewport: Rect,
    tint: [u8; 3],
    opacity: u8,
    weight: impl Fn([f64; 2]) -> f32,
) {
    let visible = image.intersect(viewport).intersect(ui.clip_rect());
    if visible.is_positive() {
        ui.painter().with_clip_rect(visible).add(Shape::mesh(mesh(
            image,
            visible,
            ui.ctx().pixels_per_point(),
            tint,
            opacity,
            weight,
        )));
    }
}

fn mesh(
    image: Rect,
    visible: Rect,
    ppp: f32,
    tint: [u8; 3],
    opacity: u8,
    weight: impl Fn([f64; 2]) -> f32,
) -> Mesh {
    // Four physical pixels per cell, capped independently on each axis.
    let nx = (visible.width() * ppp / 4.).ceil().clamp(1., 512.) as u32;
    let ny = (visible.height() * ppp / 4.).ceil().clamp(1., 512.) as u32;
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(((nx + 1) * (ny + 1)) as usize);
    mesh.indices.reserve((nx * ny * 6) as usize);
    for y in 0..=ny {
        for x in 0..=nx {
            let pos = Pos2::new(
                visible.left() + visible.width() * x as f32 / nx as f32,
                visible.top() + visible.height() * y as f32 / ny as f32,
            );
            let uv = [
                ((pos.x - image.left()) / image.width()) as f64,
                ((pos.y - image.top()) / image.height()) as f64,
            ];
            let alpha = (weight(uv).clamp(0., 1.) * opacity as f32).round() as u8;
            mesh.colored_vertex(
                pos,
                Color32::from_rgba_unmultiplied(tint[0], tint[1], tint[2], alpha),
            );
        }
    }
    for y in 0..ny {
        for x in 0..nx {
            let a = y * (nx + 1) + x;
            mesh.add_triangle(a, a + 1, a + nx + 1);
            mesh.add_triangle(a + 1, a + nx + 2, a + nx + 1);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_interpolates_without_tile_edges_and_tracks_zoomed_coordinates() {
        let image = Rect::from_min_max(Pos2::new(-100., -50.), Pos2::new(300., 150.));
        let visible = Rect::from_min_max(Pos2::ZERO, Pos2::new(100., 100.));
        let m = mesh(image, visible, 2., [40, 180, 240], 85, |uv| uv[0] as f32);
        assert!(m.is_valid());
        assert_eq!(m.vertices.first().unwrap().color.a(), 21);
        assert_eq!(m.vertices.last().unwrap().color.a(), 43);
        for tri in m.indices.as_chunks::<3>().0 {
            let alpha: Vec<_> = tri
                .iter()
                .map(|i| m.vertices[*i as usize].color.a())
                .collect();
            assert!(alpha.iter().max().unwrap() - alpha.iter().min().unwrap() <= 1);
        }
        assert!(m.vertices.iter().all(|v| visible.contains(v.pos)));
    }

    #[test]
    fn spatial_and_range_selections_keep_intermediate_coverage() {
        use tr_core::editing::masks::{Mask, Shape as MaskShape};
        let r = Rect::from_min_max(Pos2::ZERO, Pos2::new(800., 400.));
        for shape in [
            MaskShape::Linear,
            MaskShape::Radial,
            MaskShape::Brush,
            MaskShape::Luminance,
            MaskShape::Hue,
        ] {
            let mask = Mask {
                shape,
                points: vec![[0.3, 0.5], [0.7, 0.5]],
                ..Default::default()
            };
            let m = mesh(r, r, 1., [40, 180, 240], 85, |uv| {
                let t = uv[0] as f32;
                let guide = if shape == MaskShape::Hue {
                    [1. - t, 1., 0.]
                } else {
                    [t; 3]
                };
                mask.weight(uv.map(|v| v as f32), guide, 0.5)
            });
            let levels: std::collections::BTreeSet<_> =
                m.vertices.iter().map(|v| v.color.a()).collect();
            assert!(levels.len() > 20, "{shape:?}: {levels:?}");
            assert!(m.is_valid());
        }
    }

    #[test]
    fn transparent_endpoints_and_cost_are_bounded() {
        let r = Rect::from_min_max(Pos2::ZERO, Pos2::new(10000., 10000.));
        let m = mesh(r, r, 2., [230, 65, 75], 65, |uv| uv[0] as f32);
        assert_eq!(m.vertices.len(), 513 * 513);
        assert_eq!(m.vertices[0].color, Color32::TRANSPARENT);
        assert_eq!(m.vertices[512].color.a(), 65);
    }
}
