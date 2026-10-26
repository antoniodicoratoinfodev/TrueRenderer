use super::*;

pub(super) struct Vignette {
    pub source: u64,
    pub amount: f32,
    pub gains: Vec<f32>,
    pub lease: Lease,
}

pub(super) struct Prepared<'a> {
    recipe: &'a EditRecipe,
    parameters: Parameters,
    gains: [f32; 3],
    geometry: Geometry,
    full: [u32; 2],
}
impl<'a> Prepared<'a> {
    pub(super) fn new(
        recipe: &'a EditRecipe,
        gains: [f32; 3],
        vignette: f32,
        full: [u32; 2],
    ) -> Self {
        Self {
            recipe,
            parameters: Parameters::new(recipe),
            gains,
            geometry: Geometry {
                vignette,
                ..Default::default()
            },
            full,
        }
    }
    #[inline]
    pub(super) fn apply(&self, x: usize, y: usize, mut p: Pixel) -> Pixel {
        if self.gains != [1.; 3] {
            for (v, g) in p[..3].iter_mut().zip(self.gains) {
                *v *= g;
            }
        }
        if self.geometry.vignette != 0. {
            self.geometry.optical_color(
                &mut p,
                (x as f32 + 0.5) / self.full[0] as f32,
                (y as f32 + 0.5) / self.full[1] as f32,
            );
        }
        self.recipe.transform_prepared(p, &self.parameters)
    }
}

pub(super) struct Input<'a> {
    source: &'a LinearImage,
    point: Prepared<'a>,
    vignette: Option<&'a [f32]>,
}
impl<'a> Input<'a> {
    pub(super) fn new(
        source: &'a LinearImage,
        recipe: &'a EditRecipe,
        gains: [f32; 3],
        vignette: f32,
        cached: Option<&'a [f32]>,
    ) -> Self {
        Self {
            source,
            point: Prepared::new(recipe, gains, vignette, [source.width, source.height]),
            vignette: cached,
        }
    }
}
impl detail::Input for Input<'_> {
    fn cache_region(&self) -> bool {
        true
    }
    fn width(&self) -> u32 {
        self.source.width
    }
    fn height(&self) -> u32 {
        self.source.height
    }
    #[inline]
    fn pixel(&self, x: usize, y: usize) -> Pixel {
        let index = y * self.source.width as usize + x;
        let mut p = self.source.pixels[index];
        if let Some(vignette) = self.vignette {
            if self.point.gains != [1.; 3] {
                for (v, g) in p[..3].iter_mut().zip(self.point.gains) {
                    *v *= g;
                }
            }
            if p[3] > 0. {
                for v in &mut p[..3] {
                    *v *= vignette[index];
                }
            }
            self.point
                .recipe
                .transform_prepared(p, &self.point.parameters)
        } else {
            self.point.apply(x, y, p)
        }
    }
}
