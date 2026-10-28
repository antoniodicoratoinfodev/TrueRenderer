use super::*;
use image::ImageEncoder;
#[derive(Clone, Copy, PartialEq)]
pub(super) enum ImageFormat {
    Png16,
    Png8,
    Jpeg,
}
impl ImageFormat {
    pub(super) fn extension(self) -> &'static str {
        if self == Self::Jpeg { "jpg" } else { "png" }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Png16 => "PNG · 16 bit",
            Self::Png8 => "PNG · 8 bit",
            Self::Jpeg => "JPEG · sRGB",
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct ImageOptions {
    pub format: ImageFormat,
    pub quality: u8,
    pub edge: u32,
    pub space: ColorSpace,
    pub resolution: Option<tr_core::protocol::Resolution>,
}
impl ImageOptions {
    pub fn for_document(doc: &Document, format: ImageFormat, quality: u8, edge: u32) -> Self {
        Self {
            format,
            quality,
            edge,
            space: doc.color_space,
            resolution: doc.physical_resolution(),
        }
    }
}
impl Editor {
    pub(super) fn export_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Progetto modificabile");
        ui.label("Salva livelli, immagini, selezioni, maschere, regolazioni e trasformazioni in un file .truerenderer.");
        ui.small("Le immagini sono incorporate: puoi trasferire il progetto fra Windows e Mac. Lo sviluppo sensore RAW è incorporato nei pixel; le regolazioni fotografiche restano modificabili.");
        ui.add_enabled_ui(self.doc.is_some(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Salva progetto").clicked() {
                    self.save(false);
                }
                if ui.button("Salva con nome…").clicked() {
                    self.save(true);
                }
                if ui.button("Esporta copia del progetto…").clicked() {
                    self.export_native();
                }
            });
        });
        if let Some(path) = &self.path {
            ui.label(path.display().to_string());
        }
        ui.separator();
        ui.heading("Immagine finita");
        ui.label("Esporta il risultato visibile della composizione.");
        egui::ComboBox::from_id_salt("document-export-format")
            .selected_text(self.export_format.label())
            .show_ui(ui, |ui| {
                for format in [ImageFormat::Png16, ImageFormat::Png8, ImageFormat::Jpeg] {
                    ui.selectable_value(&mut self.export_format, format, format.label());
                }
            });
        if self.export_format == ImageFormat::Jpeg {
            ui.add(egui::Slider::new(&mut self.export_quality, 1..=100).text("Qualità JPEG"));
            ui.small("sRGB, compressione con perdita. Trasparenza composta su bianco.");
        } else {
            ui.small("Trasparenza conservata. PNG 8 bit usa sRGB.");
        }
        ui.horizontal(|ui| {
            ui.label("Lato lungo massimo · px");
            ui.add(egui::DragValue::new(&mut self.export_edge).range(0..=65536));
        });
        ui.small("0 = dimensioni originali. Riduce solo la copia esportata; il progetto conserva tutti i pixel.");
        if let Some(doc) = &self.doc {
            let mut space = self.export_space.unwrap_or(doc.color_space);
            ui.horizontal(|ui| {
                ui.label("Profilo PNG16");
                let changed = ui
                    .selectable_value(&mut space, ColorSpace::Srgb, "sRGB")
                    .changed();
                let changed = ui
                    .selectable_value(&mut space, ColorSpace::LinearRec2020, "Rec.2020 lineare")
                    .changed()
                    || changed;
                if changed {
                    self.export_space = Some(space);
                }
            });
            let mut custom = self.export_resolution.is_some();
            if ui
                .checkbox(&mut custom, "Scegli DPI/PPI per la copia esportata")
                .changed()
            {
                self.export_resolution = custom.then(|| {
                    doc.physical_resolution()
                        .unwrap_or(tr_core::protocol::Resolution { x: 300., y: 300. })
                });
            }
            if let Some(resolution) = &mut self.export_resolution {
                ui.horizontal(|ui| {
                    ui.label("PPI X / Y");
                    ui.add(egui::DragValue::new(&mut resolution.x).range(1. ..=9600.));
                    ui.add(egui::DragValue::new(&mut resolution.y).range(1. ..=9600.));
                });
            } else {
                ui.small(match doc.physical_resolution() {
                    Some(r) => format!("Risoluzione del documento: {:.2} × {:.2} PPI", r.x, r.y),
                    None => {
                        "DPI/PPI non dichiarati nella sorgente: nessun valore imposto all'export."
                            .into()
                    }
                });
            }
            ui.small("Dimensioni, DPI/PPI e profilo di export cambiano solo la copia esportata.");
        }
        if ui
            .add_enabled(self.doc.is_some(), egui::Button::new("Esporta immagine…"))
            .clicked()
        {
            self.export_image();
        }
    }
    pub(super) fn export_native(&mut self) {
        let Some(doc) = &self.doc else {
            return;
        };
        if let Some(mut path) = rfd::FileDialog::new()
            .set_directory(&self.directory)
            .add_filter("TrueRenderer", &["truerenderer"])
            .set_file_name("Copia.truerenderer")
            .save_file()
        {
            if path.extension().is_none() {
                path.set_extension("truerenderer");
            }
            match tr_store::document::save(&path, doc) {
                Ok(()) => {
                    self.error.clear();
                    self.notice = format!("Copia del progetto salvata: {}", path.display());
                    self.published.push_back(path);
                }
                Err(e) => self.error = e.to_string(),
            }
        }
    }
    pub(super) fn export_image(&mut self) {
        let Some(doc) = &self.doc else {
            return;
        };
        let Some(mut path) = rfd::FileDialog::new()
            .set_directory(&self.directory)
            .add_filter(
                self.export_format.label(),
                &[self.export_format.extension()],
            )
            .set_file_name(format!("Composizione.{}", self.export_format.extension()))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension(self.export_format.extension());
        }
        let result = (|| -> Result<()> {
            ensure!(
                !path.exists(),
                "Scegliere un nuovo nome: l'export non sostituisce file esistenti"
            );
            let mut temp = tempfile::NamedTempFile::new_in(
                path.parent().unwrap_or(std::path::Path::new(".")),
            )?;
            encode_with_options(
                doc,
                ImageOptions {
                    format: self.export_format,
                    quality: self.export_quality,
                    edge: self.export_edge,
                    space: self.export_space.unwrap_or(doc.color_space),
                    resolution: self.export_resolution.or_else(|| doc.physical_resolution()),
                },
                temp.as_file_mut(),
            )?;
            temp.as_file().sync_all()?;
            temp.persist_noclobber(&path).map_err(|e| e.error)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.error.clear();
                self.notice = format!("Immagine esportata: {}", path.display());
                self.published.push_back(path);
            }
            Err(e) => self.error = e.to_string(),
        }
    }
}
pub(super) fn encode_image(
    doc: &Document,
    format: ImageFormat,
    quality: u8,
    edge: u32,
    output: &mut impl std::io::Write,
) -> Result<()> {
    encode_with_options(
        doc,
        ImageOptions::for_document(doc, format, quality, edge),
        output,
    )
}
pub(super) fn encode_with_options(
    doc: &Document,
    options: ImageOptions,
    output: &mut impl std::io::Write,
) -> Result<()> {
    let ImageOptions {
        format,
        quality,
        edge,
        space,
        resolution,
    } = options;
    if let Some(r) = resolution {
        r.validate()?;
    }
    let image = doc.render()?;
    let image = if edge > 0 { image.reduced(edge) } else { image };
    if format == ImageFormat::Jpeg {
        let pixels: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|p| {
                let white = tr_core::color::rec2020_to_linear_srgb([
                    p[0] + 1. - p[3],
                    p[1] + 1. - p[3],
                    p[2] + 1. - p[3],
                ]);
                white
                    .map(|v| (tr_core::color::linear_to_srgb(v).clamp(0., 1.) * 255.).round() as u8)
            })
            .collect();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(output, quality);
        if let Some(r) = resolution {
            encoder.set_pixel_density(image::codecs::jpeg::PixelDensity {
                density: (r.x.round() as u16, r.y.round() as u16),
                unit: image::codecs::jpeg::PixelDensityUnit::Inches,
            });
            encoder.set_exif_metadata(resolution_exif(r))?;
        }
        encoder.set_icc_profile(tr_core::export::profile(false)?)?;
        encoder.encode(
            &pixels,
            image.width,
            image.height,
            image::ExtendedColorType::Rgb8,
        )?;
    } else {
        encode_png_image(
            &image,
            space,
            resolution,
            format == ImageFormat::Png8,
            output,
        )?;
    }
    Ok(())
}

// Standard TIFF IFD in APP1, preserving fractional/asymmetric PPI beyond JFIF integers.
fn resolution_exif(r: tr_core::protocol::Resolution) -> Vec<u8> {
    let mut bytes = b"II\x2a\0\x08\0\0\0".to_vec();
    bytes.extend_from_slice(&3u16.to_le_bytes());
    for (tag, kind, value) in [(282u16, 5u16, 50u32), (283, 5, 58), (296, 3, 2)] {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for ppi in [r.x, r.y] {
        bytes.extend_from_slice(&((ppi * 10000.).round() as u32).to_le_bytes());
        bytes.extend_from_slice(&10000u32.to_le_bytes());
    }
    bytes
}
