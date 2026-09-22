use super::*;
#[derive(Default)]
pub(super) struct ScienceUi {
    id: String,
    coordinates: [u32; 2],
    pending: bool,
    result: String,
    stretch: tr_core::science::Stretch,
}
impl TrueRenderer {
    pub(super) fn science_result(
        &mut self,
        id: String,
        result: Result<tr_core::science::Sample, String>,
    ) {
        if self.science.id != id {
            return;
        }
        self.science.pending = false;
        self.science.result = match result {
            Ok(s) => localized_format!(
                self.cache_settings.language,
                "HDU {} · ({},{})\nMemorizzato: {:?}\nFisico: {:?}\nValidità: {}",
                "HDU {} · ({},{})\nStored: {:?}\nPhysical: {:?}\nValidity: {}",
                s.hdu,
                s.x,
                s.y,
                s.stored,
                s.physical,
                s.validity
            ),
            Err(e) => e,
        };
    }
    pub(super) fn science_inspector(&mut self, ui: &mut egui::Ui, item: &Item, info: &RasterInfo) {
        let lang = self.cache_settings.language;
        let meta = info.scientific.as_ref().unwrap();
        if self.science.id != item.id {
            self.science.id = item.id.clone();
            self.science.coordinates = [0, 0];
            self.science.pending = false;
            self.science.result.clear();
        }
        ui.heading(lang.text("FITS · dati scientifici"));
        ui.label(format!(
            "HDU {} · {}×{} · BITPIX {}",
            meta.hdu, info.source_width, info.source_height, meta.bitpix
        ));
        ui.label(lang.text("Prima immagine 2D idonea, sola lettura. Selezione di altri HDU, cubi, tabelle, compressione e WCS non disponibili."));
        ui.label(localized_format!(
            lang,
            "Unità: {}\nBSCALE={} · BZERO={}",
            "Unit: {}\nBSCALE={} · BZERO={}",
            meta.unit,
            meta.bscale,
            meta.bzero
        ));
        ui.label(localized_format!(
            lang,
            "Validi: {} · mancanti/non finiti: {}\nMin {:?} · Max {:?}",
            "Valid: {} · missing/nonfinite: {}\nMin {:?} · Max {:?}",
            meta.valid,
            meta.invalid,
            meta.minimum,
            meta.maximum
        ));
        let bins: [u32; 256] = std::array::from_fn(|i| meta.histogram[i] as u32);
        tr_render::histogram(ui, &[bins; 3]);
        ui.small(lang.text("Istogramma scalare del piano nativo completo, prima dello stretch; min/max nelle unità dichiarate."));
        ui.separator();
        ui.label(lang.text("Trasformata di vista (globale FITS)"));
        ui.small(lang.text("Nero/bianco nella scala min–max normalizzata; non modifica i campioni. Piano costante → grigio medio, invalidi → sfondo."));
        ui.add(
            egui::Slider::new(&mut self.science.stretch.black, 0. ..=0.99).text(lang.text("Nero")),
        );
        ui.add(
            egui::Slider::new(&mut self.science.stretch.white, 0.01..=1.).text(lang.text("Bianco")),
        );
        self.science.stretch.white = self
            .science
            .stretch
            .white
            .max(self.science.stretch.black + 0.001);
        ui.checkbox(&mut self.science.stretch.asinh, "Stretch asinh (10×)");
        if ui.button(lang.text("Reset · min/max lineare")).clicked() {
            self.science.stretch = Default::default();
        }
        self.presenter.set_scientific_stretch(self.science.stretch);
        ui.small(lang.text("Proxy fp32 e copertura separata; media sui validi, poi stretch. Calcolo FITS su CPU anche in modalità GPU; presentazione 8/10/16 float comune. Non fotometria."));
        ui.separator();
        ui.label(lang.text("Campione nativo (lettura isolata su richiesta)"));
        ui.horizontal(|ui| {
            ui.label("x");
            ui.add(
                egui::DragValue::new(&mut self.science.coordinates[0])
                    .range(0..=info.source_width - 1),
            );
            ui.label("y");
            ui.add(
                egui::DragValue::new(&mut self.science.coordinates[1])
                    .range(0..=info.source_height - 1),
            );
        });
        if ui
            .add_enabled(
                !self.science.pending,
                egui::Button::new(lang.text("Leggi valore originale")),
            )
            .clicked()
        {
            self.science.pending = self.request(Request::ScientificSample(
                crate::photo_export::ScientificJob {
                    item: item.clone(),
                    x: self.science.coordinates[0],
                    y: self.science.coordinates[1],
                    generation: self.generation,
                },
            ));
        }
        if self.science.pending {
            ui.spinner();
        }
        ui.label(&self.science.result);
        ui.small(lang.text("Coordinate 0-based; asse 1 orizzontale, asse 2 verso il basso. Il campione proviene dai byte originali, non da mip/stretch. Valore fisico calcolato in f64 una sola volta; NaN/Inf/BLANK distinti."));
    }
}
