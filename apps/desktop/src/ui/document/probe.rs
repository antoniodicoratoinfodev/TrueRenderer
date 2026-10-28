//! Explicit isolated native probe; does not run in ordinary sessions.
use super::*;
impl Editor {
    pub fn verify_native_source(&mut self, root: &std::path::Path) -> serde_json::Value {
        let result = (|| -> Result<serde_json::Value> {
            let expected: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("source.json"))?)?;
            let doc = self
                .doc
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Documento assente"))?;
            ensure!(
                doc.layers.len() == 1 && self.source_id.is_some(),
                "Importazione fotografia assente"
            );
            ensure!(
                u64::from(doc.width) == expected["width"].as_u64().unwrap()
                    && u64::from(doc.height) == expected["height"].as_u64().unwrap(),
                "Pixel nativi diversi dalla sorgente: {} × {}",
                doc.width,
                doc.height
            );
            let density = doc.physical_resolution();
            if expected["ppi"].is_null() {
                ensure!(density.is_none(), "DPI inventati");
            } else {
                let r = density.ok_or_else(|| anyhow::anyhow!("DPI della sorgente mancanti"))?;
                ensure!(
                    (r.x - expected["ppi"][0].as_f64().unwrap()).abs() < 0.02
                        && (r.y - expected["ppi"][1].as_f64().unwrap()).abs() < 0.02,
                    "DPI sorgente alterati"
                );
            }
            let file = root.join("native.truerenderer");
            tr_store::document::save(&file, doc)?;
            let reopened =
                tr_store::document::open_admitted(&file, self.admission.as_ref().unwrap().bytes())?;
            ensure!(reopened == *doc, "Riapertura diversa");
            // Export uses the full native canvas by default; the comparison is independent of preview size.
            let mut png = std::fs::File::create(root.join("native.png"))?;
            export::encode_image(doc, export::ImageFormat::Png16, 95, 0, &mut png)?;
            let reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(
                root.join("native.png"),
            )?))
            .read_info()?;
            ensure!(
                [reader.info().width, reader.info().height] == [doc.width, doc.height],
                "Export ridimensionato"
            );
            if let Some(r) = density {
                let p = reader
                    .info()
                    .pixel_dims
                    .ok_or_else(|| anyhow::anyhow!("DPI export assenti"))?;
                ensure!(
                    (p.xppu as f64 * 0.0254 - r.x).abs() < 0.02
                        && (p.yppu as f64 * 0.0254 - r.y).abs() < 0.02,
                    "DPI export alterati"
                );
            } else {
                ensure!(reader.info().pixel_dims.is_none(), "DPI export inventati");
            }
            Ok(
                serde_json::json!({"passed":true,"source":expected,"canvas":[doc.width,doc.height],"density":density,"storage_bytes":doc.storage_bytes(),"admission_bytes":self.admission.as_ref().unwrap().bytes(),"native_export":true,"roundtrip_exact":true,"format_version":doc.version}),
            )
        })();
        self.dirty = false;
        match result {
            Ok(r) => r,
            Err(e) => serde_json::json!({"passed":false,"error":e.to_string()}),
        }
    }
    pub fn verify_workflow(&mut self, root: &std::path::Path) -> serde_json::Value {
        let result = (|| -> Result<serde_json::Value> {
            ensure!(
                self.source_id.is_some(),
                "Photo was not captured through the viewer"
            );
            let base = self
                .doc
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing document"))?
                .clone();
            ensure!(base.layers.len() == 1, "Expected photo at level zero");
            self.edit(|d, _| d.duplicate(0));
            ensure!(self.error.is_empty(), "{}", self.error);
            self.select_layer(1);
            self.begin_stroke();
            self.doc.as_mut().unwrap().layers[1].transform.x = 13.;
            self.doc.as_mut().unwrap().layers[1].transform.y = 9.;
            self.end_stroke();
            self.undo_edit();
            ensure!(
                self.doc.as_ref().unwrap().layers[1].transform.x == 0.,
                "Move undo failed"
            );
            self.redo_edit();
            let w = base.width as f32;
            let h = base.height as f32;
            self.edit(|d, _| {
                d.select_polygon(
                    &[
                        [w * 0.1, h * 0.1],
                        [w * 0.4, h * 0.1],
                        [w * 0.4, h * 0.4],
                        [w * 0.1, h * 0.4],
                    ],
                    SelectionMode::Replace,
                    1.,
                )
            });
            self.edit(|d, i| d.copy_selection(i));
            ensure!(self.error.is_empty(), "{}", self.error);
            self.select_layer(2);
            self.edit(|d, i| {
                d.layers[i].transform.x += 20.;
                d.layers[i].recipe.exposure_ev = 0.4;
                Ok(())
            });
            let composed = self.doc.as_ref().unwrap();
            ensure!(composed.layers[0] == base.layers[0], "Base changed");
            let path = root.join("workflow.truerenderer");
            tr_store::document::save(&path, composed)?;
            let reopened = tr_store::document::open(&path)?;
            ensure!(reopened == *composed, "Project round trip differs");
            ensure!(
                reopened.render()?.pixels == composed.render()?.pixels,
                "Render differs after reopening"
            );
            for (format, name) in [
                (export::ImageFormat::Png16, "workflow.png"),
                (export::ImageFormat::Jpeg, "workflow.jpg"),
            ] {
                let mut file = std::fs::File::create(root.join(name))?;
                export::encode_image(composed, format, 95, 320, &mut file)?;
            }
            Ok(
                serde_json::json!({"passed":true,"layers":composed.layers.len(),"base_preserved":true,"move_undo_redo":true,"selection_copy":true,"roundtrip_exact":true,"png_jpeg_export":true}),
            )
        })();
        // This probe only owns its isolated project; suppress exit save prompts.
        self.dirty = false;
        match result {
            Ok(report) => report,
            Err(e) => serde_json::json!({"passed":false,"error":e.to_string()}),
        }
    }
}
