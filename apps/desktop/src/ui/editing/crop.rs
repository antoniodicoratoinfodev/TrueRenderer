//! A crop session never changes the recipe until explicit confirmation.
use super::*;
const FULL: [f32; 4] = [0., 0., 1., 1.];

#[derive(Clone, Copy, PartialEq)]
enum RatioChoice {
    Free,
    Original,
    Preset(u8, u8),
    Custom,
    Current,
}
impl RatioChoice {
    fn label(self, lang: crate::i18n::Language) -> String {
        match self {
            Self::Free => lang.text("Libero").into(),
            Self::Original => lang.text("Originale").into(),
            Self::Preset(w, h) => format!("{w}:{h}"),
            Self::Custom => lang.text("Personalizzato").into(),
            Self::Current => lang.text("Rapporto attuale").into(),
        }
    }
}

pub(in crate::ui) struct Session {
    pub item: Item,
    initial: EditRecipe,
    pub rect: [f32; 4],
    ratio: Option<f32>,
    custom: [f32; 2],
    choice: RatioChoice,
    grid: bool,
    move_image: bool,
    drag: Option<([f32; 4], egui::Pos2, [i8; 2])>,
    saving: bool,
    error: Option<String>,
    drag_center: [f32; 2],
}
impl Session {
    fn new(item: Item, initial: EditRecipe) -> Self {
        let rect = initial.advanced.as_ref().map_or(FULL, |a| a.geometry.crop);
        Self {
            item,
            initial,
            rect,
            ratio: None,
            custom: [3., 2.],
            choice: RatioChoice::Free,
            grid: true,
            move_image: false,
            drag: None,
            saving: false,
            error: None,
            drag_center: [0.5, 0.5],
        }
    }
    fn recipe(&self) -> EditRecipe {
        let mut result = self.initial.clone();
        if self.rect
            != self
                .initial
                .advanced
                .as_ref()
                .map_or(FULL, |a| a.geometry.crop)
        {
            result
                .advanced
                .get_or_insert_with(Default::default)
                .geometry
                .crop = self.rect;
            result.require_process(3);
        }
        result
    }
    fn center(&mut self) {
        let [l, t, r, b] = self.rect;
        self.rect = [
            (1. - r + l) / 2.,
            (1. - b + t) / 2.,
            (1. + r - l) / 2.,
            (1. + b - t) / 2.,
        ];
    }
    fn current_ratio(&self, size: [u32; 2]) -> f32 {
        (self.rect[2] - self.rect[0]) / (self.rect[3] - self.rect[1]) * size[0] as f32
            / size[1] as f32
    }
    fn choose_ratio(&mut self, choice: RatioChoice, size: [u32; 2]) {
        self.choice = choice;
        match choice {
            RatioChoice::Free => self.ratio = None,
            RatioChoice::Original => self.aspect(size[0] as f32 / size[1] as f32, size),
            RatioChoice::Preset(w, h) => self.aspect(w as f32 / h as f32, size),
            RatioChoice::Custom => self.aspect(self.custom[0] / self.custom[1], size),
            RatioChoice::Current => self.ratio = Some(self.current_ratio(size)),
        }
    }
    fn reset(&mut self) {
        self.rect = FULL;
        self.ratio = None;
        self.choice = RatioChoice::Free;
    }
    fn inverted_rect(&self, size: [u32; 2]) -> Option<[f32; 4]> {
        let ratio = self.current_ratio(size);
        if (ratio - 1.).abs() < 0.0001 {
            return None;
        }
        let [l, t, r, b] = self.rect;
        let source_ratio = size[0] as f32 / size[1] as f32;
        // Exchange physical width/height, preserving area when the source permits it.
        let w = (b - t) / source_ratio;
        let h = (r - l) * source_ratio;
        let scale = (1. / w).min(1. / h).min(1.);
        if w * scale < 0.01 || h * scale < 0.01 {
            return None;
        }
        Some(fit_center(
            [(l + r) / 2., (t + b) / 2.],
            [w * scale, h * scale],
        ))
    }
    fn invert_orientation(&mut self, size: [u32; 2]) {
        let ratio = self.current_ratio(size);
        let Some(rect) = self.inverted_rect(size) else {
            return;
        };
        self.rect = rect;
        self.choice = match self.choice {
            RatioChoice::Preset(w, h) => RatioChoice::Preset(h, w),
            RatioChoice::Custom => {
                self.custom.swap(0, 1);
                RatioChoice::Custom
            }
            RatioChoice::Original => RatioChoice::Current,
            other => other,
        };
        self.ratio = self.ratio.map(|_| 1. / ratio);
    }
    fn keyboard_move(&mut self, delta: [f32; 2], transform: &mut ViewTransform) {
        let start = self.rect;
        let sign = if self.move_image { -1. } else { 1. };
        self.rect = dragged(start, delta.map(|v| v * sign), [0, 0], None);
        if self.move_image {
            for (axis, previous) in start.iter().enumerate().take(2) {
                transform.center[axis] += self.rect[axis] - previous;
            }
        }
    }
    fn resize(&mut self, factor: f32) {
        let [l, t, r, b] = self.rect;
        let w = r - l;
        let h = b - t;
        let factor = factor.clamp((0.01 / w).max(0.01 / h), (1. / w).min(1. / h));
        self.rect = fit_center([(l + r) / 2., (t + b) / 2.], [w * factor, h * factor]);
    }
    fn ratio_options(&mut self, ui: &mut egui::Ui, size: [u32; 2], lang: crate::i18n::Language) {
        let portrait = self.current_ratio(size) < 1.;
        let mut choices = vec![RatioChoice::Free, RatioChoice::Original];
        for (w, h) in [(1, 1), (3, 2), (4, 3), (5, 4), (16, 9)] {
            choices.push(if portrait {
                RatioChoice::Preset(h, w)
            } else {
                RatioChoice::Preset(w, h)
            });
        }
        choices.push(RatioChoice::Custom);
        if self.choice == RatioChoice::Current {
            choices.push(RatioChoice::Current);
        }
        for choice in choices {
            if ui
                .selectable_label(self.choice == choice, choice.label(lang))
                .clicked()
            {
                self.choose_ratio(choice, size);
                ui.close();
            }
        }
    }
    fn valid(&self, rect: [f32; 4], native: [u32; 2]) -> bool {
        let mut g = self
            .initial
            .advanced
            .as_ref()
            .map(|a| a.geometry.clone())
            .unwrap_or_default();
        g.crop = rect;
        // Stored edges and their span use f32; the inverse map uses f64.
        // Keep their rounding error from turning a source edge into an invalid border.
        let epsilon = 2. * f64::from(f32::EPSILON);
        (0..=64).all(|i| {
            let t = i as f64 / 64.;
            [[t, 0.], [t, 1.], [0., t], [1., t]].into_iter().all(|p| {
                g.source_point(p, native)
                    .is_some_and(|p| p.iter().all(|v| (-epsilon..=1. + epsilon).contains(v)))
            })
        })
    }
    fn constrain(&mut self, native: [u32; 2]) {
        if self.valid(self.rect, native) {
            return;
        }
        let [l, t, r, b] = self.rect;
        let center = [(l + r) / 2., (t + b) / 2.];
        let size = [r - l, b - t];
        let minimum = (0.01 / size[0]).max(0.01 / size[1]);
        let candidate = |factor| fit_center(center, [size[0] * factor, size[1] * factor]);
        if !self.valid(candidate(minimum), native) {
            return;
        }
        let (mut low, mut high) = (minimum, 1.);
        for _ in 0..24 {
            let mid = (low + high) * 0.5;
            if self.valid(candidate(mid), native) {
                low = mid;
            } else {
                high = mid;
            }
        }
        self.rect = candidate(low);
    }
    fn aspect(&mut self, ratio: f32, size: [u32; 2]) {
        let normalized = (ratio / (size[0] as f32 / size[1] as f32)).clamp(0.01, 100.);
        let [l, t, r, b] = self.rect;
        let w = (r - l)
            .min((b - t) * normalized)
            .max(0.01_f32.max(0.01 * normalized));
        let h = w / normalized;
        self.rect = fit_center([(l + r) / 2., (t + b) / 2.], [w, h]);
        self.ratio = Some(normalized * size[0] as f32 / size[1] as f32);
    }
}
fn fit_center(c: [f32; 2], size: [f32; 2]) -> [f32; 4] {
    let [w, h] = size.map(|v| v.clamp(0.01, 1.));
    let x = (c[0] - w / 2.).clamp(0., 1. - w);
    let y = (c[1] - h / 2.).clamp(0., 1. - h);
    [x, y, x + w, y + h]
}
fn dragged(start: [f32; 4], delta: [f32; 2], handle: [i8; 2], ratio: Option<f32>) -> [f32; 4] {
    if delta == [0., 0.] {
        return start;
    }
    let [l, t, r, b] = start;
    if handle == [0, 0] {
        let x = delta[0].clamp(-l, 1. - r);
        let y = delta[1].clamp(-t, 1. - b);
        return [l + x, t + y, r + x, b + y];
    }
    let mut next = start;
    for axis in 0..2 {
        if handle[axis] < 0 {
            next[axis] = (start[axis] + delta[axis]).clamp(0., start[axis + 2] - 0.01);
        }
        if handle[axis] > 0 {
            next[axis + 2] = (start[axis + 2] + delta[axis]).clamp(start[axis] + 0.01, 1.);
        }
    }
    if next == start {
        return start;
    }
    if let Some(ratio) = ratio {
        let mut w = next[2] - next[0];
        let mut h = next[3] - next[1];
        if handle[0] == 0 {
            w = h * ratio;
        } else {
            h = w / ratio;
        }
        let anchor = [
            if handle[0] < 0 {
                r
            } else if handle[0] > 0 {
                l
            } else {
                (l + r) / 2.
            },
            if handle[1] < 0 {
                b
            } else if handle[1] > 0 {
                t
            } else {
                (t + b) / 2.
            },
        ];
        let max_w = if handle[0] < 0 {
            r
        } else if handle[0] > 0 {
            1. - l
        } else {
            2. * anchor[0].min(1. - anchor[0])
        };
        let max_h = if handle[1] < 0 {
            b
        } else if handle[1] > 0 {
            1. - t
        } else {
            2. * anchor[1].min(1. - anchor[1])
        };
        let factor = (max_w / w).min(max_h / h).min(1.);
        w *= factor;
        h *= factor;
        if w < 0.01 || h < 0.01 {
            return start;
        }
        next = [
            anchor[0]
                - if handle[0] < 0 {
                    w
                } else if handle[0] == 0 {
                    w / 2.
                } else {
                    0.
                },
            anchor[1]
                - if handle[1] < 0 {
                    h
                } else if handle[1] == 0 {
                    h / 2.
                } else {
                    0.
                },
            0.,
            0.,
        ];
        next[2] = next[0] + w;
        next[3] = next[1] + h;
    }
    next
}
impl TrueRenderer {
    fn crop_native_size(&self) -> Option<[u32; 2]> {
        let id = &self.editing.crop.as_ref()?.item.id;
        self.cache
            .iter()
            .find(|((photo, _), _)| photo == id)
            .map(|(_, c)| c.pyramid.source_size())
    }
    fn crop_oriented_size(&self, mut size: [u32; 2]) -> [u32; 2] {
        if self.editing.crop.as_ref().is_some_and(|s| {
            s.initial
                .advanced
                .as_ref()
                .is_some_and(|a| a.geometry.quarter_turns % 2 == 1)
        }) {
            size.swap(0, 1);
        }
        size
    }
    pub(in crate::ui) fn crop_keyboard(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.editing.crop else {
            return;
        };
        if s.saving
            || self.state.current.as_deref() != Some(&s.item.id)
            || self.state.view != ViewMode::Preview
        {
            return;
        }
        let modifiers = ctx.input(|i| i.modifiers);
        if modifiers.command || modifiers.alt || modifiers.ctrl {
            return;
        }
        let key = |key| ctx.input(|i| i.key_pressed(key));
        if key(egui::Key::Enter) {
            self.finish_crop(true);
            return;
        }
        let Some(size) = self
            .crop_native_size()
            .map(|size| self.crop_oriented_size(size))
        else {
            return;
        };
        let s = self.editing.crop.as_mut().unwrap();
        let step = if modifiers.shift { 10. } else { 1. };
        let delta = [
            (i32::from(key(egui::Key::ArrowRight)) - i32::from(key(egui::Key::ArrowLeft))) as f32
                * step
                / size[0] as f32,
            (i32::from(key(egui::Key::ArrowDown)) - i32::from(key(egui::Key::ArrowUp))) as f32
                * step
                / size[1] as f32,
        ];
        if delta != [0., 0.] {
            s.keyboard_move(delta, &mut self.state.transform);
        }
        if key(egui::Key::Plus) || key(egui::Key::Equals) {
            s.resize(1. + step * 0.01);
        }
        if key(egui::Key::Minus) {
            s.resize(1. / (1. + step * 0.01));
        }
        if key(egui::Key::X) {
            s.invert_orientation(size);
        }
    }
    pub(in crate::ui) fn crop_menu_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.cache_settings.language;
        let native = self.crop_native_size();
        let size = native.map(|size| self.crop_oriented_size(size));
        let Some(s) = &mut self.editing.crop else {
            return;
        };
        let mut finish = None;
        ui.strong(lang.text("Ritaglio"));
        ui.add_enabled_ui(size.is_some() && !s.saving, |ui| {
            let size = size.unwrap_or([1, 1]);
            ui.menu_button(lang.text("Proporzioni"), |ui| {
                s.ratio_options(ui, size, lang)
            });
            let mut locked = s.ratio.is_some();
            if ui
                .checkbox(&mut locked, lang.text("Blocca proporzioni"))
                .changed()
            {
                s.choose_ratio(
                    if locked {
                        RatioChoice::Current
                    } else {
                        RatioChoice::Free
                    },
                    size,
                );
            }
            if ui
                .add_enabled(
                    s.inverted_rect(size).is_some(),
                    egui::Button::new(lang.text("Inverti orientamento")).shortcut_text("X"),
                )
                .on_disabled_hover_text(
                    lang.text("Il riquadro è quadrato o troppo stretto per invertirlo."),
                )
                .clicked()
            {
                s.invert_orientation(size);
                ui.close();
            }
            ui.separator();
            ui.selectable_value(&mut s.move_image, false, lang.text("Spostare il riquadro"));
            ui.selectable_value(&mut s.move_image, true, lang.text("Spostare la foto"));
            ui.checkbox(&mut s.grid, lang.text("Griglia dei terzi"));
            if ui.button(lang.text("Ricentra ritaglio")).clicked() {
                s.center();
                self.state.transform = ViewTransform::default();
                ui.close();
            }
            if ui
                .button(lang.text("Immagine intera"))
                .on_hover_text(
                    lang.text("Rimuove solo il ritaglio. Le altre regolazioni restano applicate."),
                )
                .clicked()
            {
                s.reset();
                self.state.transform = ViewTransform::default();
                ui.close();
            }
            if let Some(native) = native
                && !s.valid(s.rect, native)
                && ui.button(lang.text("Vincola all’area valida")).clicked()
            {
                s.constrain(native);
                ui.close();
            }
        });
        ui.separator();
        if ui
            .add_enabled(
                !s.saving,
                egui::Button::new(lang.text("Applica ritaglio")).shortcut_text("Enter"),
            )
            .clicked()
        {
            finish = Some(true);
            ui.close();
        }
        if ui
            .add_enabled(
                !s.saving,
                egui::Button::new(lang.text("Annulla ritaglio")).shortcut_text("Esc"),
            )
            .clicked()
        {
            finish = Some(false);
            ui.close();
        }
        if let Some(confirm) = finish {
            self.finish_crop(confirm);
        }
    }
    pub(in crate::ui) fn crop_active(&self, id: &str) -> bool {
        self.state.view == ViewMode::Preview
            && self.state.current.as_deref() == Some(id)
            && self
                .editing
                .crop
                .as_ref()
                .is_some_and(|s| s.item.id == id && !s.saving)
    }
    pub(in crate::ui) fn view_source_size(&self, id: &str, native: [u32; 2]) -> [u32; 2] {
        self.view_recipe(id)
            .and_then(|r| r.advanced)
            .map_or(native, |a| a.geometry.output_size(native))
    }
    pub(in crate::ui) fn view_recipe(&self, id: &str) -> Option<EditRecipe> {
        if let Some(recipe) = self.look_preview_recipe(id) {
            return Some(recipe);
        }
        let mut r = self.editing.entries.get(id)?.draft.clone()?;
        if self.crop_active(id)
            && let Some(a) = r.advanced.as_mut()
        {
            a.geometry.crop = FULL;
        }
        if !self.crop_active(id) {
            self.editing.layers.preview_input(id, &mut r);
        }
        Some(r)
    }
    pub(in crate::ui) fn start_crop(&mut self, item: &Item) {
        if self.editing.crop.is_some() {
            return;
        }
        let Some(e) = self.editing.entries.get(&item.id) else {
            return;
        };
        if e.pending || e.loading || e.dirty() || e.error.is_some() || self.editing.wb_pending {
            return;
        }
        let Some(recipe) = e.draft.clone() else {
            return;
        };
        self.finish_look_preview(false);
        self.editing.crop = Some(Session::new(item.clone(), recipe));
        self.editing.show_original = false;
        self.editing.advanced = Default::default();
        self.editing.verify_final = None;
        self.clear_edit_preview();
        self.state.view = ViewMode::Preview;
        self.state.transform = ViewTransform::default();
    }
    pub(in crate::ui) fn finish_crop(&mut self, confirm: bool) {
        let Some(s) = &self.editing.crop else {
            return;
        };
        if s.saving {
            return;
        }
        if confirm
            && !self.known_items().any(|p| {
                p.id == s.item.id
                    && p.digest == s.item.digest
                    && p.observation == s.item.observation
                    && p.approved
            })
        {
            self.editing.crop.as_mut().unwrap().error =
                Some("Sorgente cambiata: bozza conservata.".into());
            return;
        }
        if confirm && s.recipe() != s.initial {
            let (id, recipe) = (s.item.id.clone(), s.recipe());
            self.apply_edit_draft(&id, recipe);
            self.editing.crop.as_mut().unwrap().saving = true;
            self.commit_edit(&id);
        } else {
            self.editing.crop = None;
        }
        self.clear_edit_preview();
        self.state.transform = ViewTransform::default();
    }
    pub(in crate::ui) fn crop_window(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.editing.crop else {
            return;
        };
        let id = s.item.id.clone();
        if s.saving
            && self
                .editing
                .entries
                .get(&id)
                .is_some_and(|e| !e.pending && !e.dirty() && e.error.is_none())
        {
            self.editing.crop = None;
            return;
        }
        let lang = self.cache_settings.language;
        let size = self
            .cache
            .iter()
            .find(|((photo, _), _)| photo == &id)
            .map(|(_, c)| c.pyramid.source_size());
        let native = size;
        let size = size.map(|mut size| {
            if s.initial
                .advanced
                .as_ref()
                .is_some_and(|a| a.geometry.quarter_turns % 2 == 1)
            {
                size.swap(0, 1);
            }
            size
        });
        let mut finish = None;
        let mut retry = false;
        let mut resume = None;
        let mut recover = false;
        let mut export = false;
        let suspended =
            self.state.current.as_deref() != Some(&id) || self.state.view != ViewMode::Preview;
        if suspended {
            self.editing.crop.as_mut().unwrap().drag = None;
        }
        egui::Window::new(lang.text("Ritaglio"))
            .id(egui::Id::new("crop-session-v2"))
            .default_pos(egui::pos2((ctx.content_rect().right() - 308.).max(0.), 120.))
            .default_width(288.)
            .resizable(false)
            .collapsible(false)
            .frame(egui::Frame::window(&ctx.style_of(ctx.theme())).inner_margin(egui::Margin::same(10)))
            .show(ctx, |ui| {
                ui.set_max_width(288.);
                ui.spacing_mut().item_spacing = egui::vec2(8., 4.);
                ui.spacing_mut().interact_size.y = 24.;
                ui.spacing_mut().button_padding = egui::vec2(8., 3.);
                let s = self.editing.crop.as_mut().unwrap();
                ui.add(egui::Label::new(egui::RichText::new(&s.item.name).small()).truncate())
                    .on_hover_text(&s.item.name);
                if let Some(error) = &s.error {
                    ui.colored_label(AMBER, lang.text(error));
                }
                if s.saving {
                    ui.horizontal(|ui| { ui.spinner(); ui.label(lang.text("Salvataggio…")); });
                    if let Some(error) = self.editing.entries.get(&id).and_then(|e| e.error.as_ref()) {
                        ui.colored_label(AMBER, error);
                    }
                    if self.editing.entries.get(&id).is_some_and(|e| !e.pending) {
                        retry = ui.button(lang.text("Riprova salvataggio")).clicked();
                        recover = ui.button(lang.text("Torna alla bozza")).clicked();
                        export = ui.button(lang.text("Esporta ricetta…")).clicked();
                    }
                    return;
                }
                ui.separator();
                if suspended {
                    ui.label(lang.text("Bozza conservata su questa foto."));
                    if ui.button(lang.text("Torna alla foto del ritaglio")).clicked() {
                        resume = Some(s.item.path.clone());
                    }
                } else {
                    if size.is_none() {
                        ui.label(lang.text("Caricamento dell’immagine…"));
                    }
                    ui.add_enabled_ui(size.is_some(), |ui| {
                        let size = size.unwrap_or([1, 1]);
                        ui.horizontal(|ui| {
                            ui.label(lang.text("Proporzioni"));
                            egui::ComboBox::from_id_salt("crop-ratio")
                                .selected_text(s.choice.label(lang))
                                .width(160.)
                                .show_ui(ui, |ui| s.ratio_options(ui, size, lang));
                        });
                        if s.choice == RatioChoice::Custom {
                            ui.horizontal(|ui| {
                                ui.label(lang.text("Rapporto"));
                                let width = ui.add(egui::DragValue::new(&mut s.custom[0])
                                    .range(0.1..=100.).speed(0.1))
                                    .on_hover_text(lang.text("Larghezza del rapporto"));
                                ui.label(":");
                                let height = ui.add(egui::DragValue::new(&mut s.custom[1])
                                    .range(0.1..=100.).speed(0.1))
                                    .on_hover_text(lang.text("Altezza del rapporto"));
                                if width.changed() || height.changed() {
                                    s.aspect(s.custom[0] / s.custom[1], size);
                                }
                            });
                            if s.ratio.is_some_and(|r| (r - s.custom[0] / s.custom[1]).abs() > 0.001) {
                                ui.small(lang.text("Rapporto limitato dall’area minima del ritaglio."));
                            }
                        }
                        ui.horizontal(|ui| {
                            let mut locked = s.ratio.is_some();
                            if ui.checkbox(&mut locked, lang.text("Blocca proporzioni")).changed() {
                                s.choose_ratio(if locked { RatioChoice::Current } else { RatioChoice::Free }, size);
                            }
                            let swap = ui.add_enabled(s.inverted_rect(size).is_some(),
                                egui::Button::new(lang.text("Inverti"))).on_hover_text(lang.text("Inverti orientamento · X"))
                                .on_disabled_hover_text(lang.text("Il riquadro è quadrato o troppo stretto per invertirlo."));
                            swap.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button,
                                swap.enabled(), lang.text("Inverti orientamento")));
                            if swap.clicked() {
                                s.invert_orientation(size);
                            }
                        });
                        ui.separator();
                        ui.label(lang.text("Trascina all’interno per"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut s.move_image, false, lang.text("Spostare il riquadro"));
                            ui.selectable_value(&mut s.move_image, true, lang.text("Spostare la foto"));
                        });
                        ui.small(lang.text("Trascina i bordi per ridimensionare."));
                        ui.checkbox(&mut s.grid, lang.text("Griglia dei terzi"));
                        ui.horizontal(|ui| {
                            if ui.button(lang.text("Ricentra ritaglio")).clicked() {
                                s.center();
                                self.state.transform = ViewTransform::default();
                            }
                            if ui.button(lang.text("Immagine intera"))
                                .on_hover_text(lang.text("Rimuove solo il ritaglio. Le altre regolazioni restano applicate.")).clicked() {
                                s.reset();
                                self.state.transform = ViewTransform::default();
                            }
                        });
                        if let Some(native) = native && !s.valid(s.rect, native) {
                            ui.small(lang.text("Bordi esterni alla sorgente: trasparenti nel risultato."));
                            if ui.button(lang.text("Vincola all’area valida")).clicked() { s.constrain(native); }
                        }
                    });
                    ui.collapsing(lang.text("Mouse e tastiera"), |ui| {
                        ui.small(lang.text("Rotella: zoom · Spazio + trascina: sposta la vista."));
                        ui.small(lang.text("Con la foto attiva: frecce per spostare, +/− per ridimensionare. Maiusc: passo maggiore."));
                        ui.small(lang.text("X: inverti orientamento · Invio: applica · Esc: annulla."));
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_sized([142., 28.], egui::Button::new(egui::RichText::new(lang.text("Applica ritaglio")).strong())
                        .fill(egui::Color32::from_gray(72))).clicked() { finish = Some(true); }
                    if ui.add_sized([130., 28.], egui::Button::new(lang.text("Annulla ritaglio")))
                        .clicked() { finish = Some(false); }
                });
            });
        if recover {
            let s = self.editing.crop.as_mut().unwrap();
            s.saving = false;
            let initial = s.initial.clone();
            self.apply_edit_draft(&id, initial);
            self.clear_edit_preview();
        }
        if export
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("crop-recipe.json")
                .save_file()
        {
            use std::io::Write;
            let recipe = self.editing.crop.as_ref().unwrap().recipe();
            let result = (|| -> anyhow::Result<()> {
                let bytes = serde_json::to_vec_pretty(&recipe)?;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                Ok(())
            })();
            self.status = match result {
                Ok(()) => lang.text("Ricetta esportata").into(),
                Err(e) => e.to_string(),
            };
        }
        if retry {
            self.commit_edit(&id);
        }
        if let Some(path) = resume {
            self.navigate(path, Some(ViewMode::Preview));
        }
        if let Some(confirm) = finish {
            self.finish_crop(confirm);
        }
    }
    pub(in crate::ui) fn crop_interaction(
        &mut self,
        ui: &egui::Ui,
        item: &Item,
        response: &egui::Response,
        native: [u32; 2],
    ) {
        if !self.crop_active(&item.id) {
            return;
        }
        let size = self.view_source_size(&item.id, native).map(|v| v as f32);
        let ppp = ui.ctx().pixels_per_point();
        let scale =
            self.state
                .transform
                .scale(size, [response.rect.width(), response.rect.height()], ppp);
        let physical = egui::vec2(
            (size[0] * scale * ppp).round(),
            (size[1] * scale * ppp).round(),
        ) / ppp;
        let top = response.rect.center()
            - egui::vec2(
                physical.x * self.state.transform.center[0],
                physical.y * self.state.transform.center[1],
            );
        let top = egui::pos2((top.x * ppp).round() / ppp, (top.y * ppp).round() / ppp);
        let full = egui::Rect::from_min_size(top, physical);
        let s = self.editing.crop.as_mut().unwrap();
        let rect = egui::Rect::from_min_max(
            top + physical * egui::vec2(s.rect[0], s.rect[1]),
            top + physical * egui::vec2(s.rect[2], s.rect[3]),
        );
        let painter = ui.painter().with_clip_rect(response.rect.intersect(full));
        for shade in [
            egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, rect.min.y)),
            egui::Rect::from_min_max(egui::pos2(full.min.x, rect.max.y), full.max),
            egui::Rect::from_min_max(egui::pos2(full.min.x, rect.min.y), rect.left_bottom()),
            egui::Rect::from_min_max(rect.right_top(), egui::pos2(full.max.x, rect.max.y)),
        ] {
            painter.rect_filled(shade, 0, egui::Color32::from_black_alpha(135));
        }
        if s.grid {
            for fraction in [1. / 3., 2. / 3.] {
                for line in [
                    [
                        egui::pos2(rect.left() + rect.width() * fraction, rect.top()),
                        egui::pos2(rect.left() + rect.width() * fraction, rect.bottom()),
                    ],
                    [
                        egui::pos2(rect.left(), rect.top() + rect.height() * fraction),
                        egui::pos2(rect.right(), rect.top() + rect.height() * fraction),
                    ],
                ] {
                    painter.line_segment(
                        line,
                        egui::Stroke::new(2., egui::Color32::from_black_alpha(100)),
                    );
                    painter.line_segment(
                        line,
                        egui::Stroke::new(0.75, egui::Color32::from_white_alpha(170)),
                    );
                }
            }
        }
        painter.rect_stroke(
            rect,
            0,
            egui::Stroke::new(3.5, egui::Color32::from_black_alpha(180)),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            rect,
            0,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        for p in [
            rect.left_top(),
            rect.center_top(),
            rect.right_top(),
            rect.left_center(),
            rect.right_center(),
            rect.left_bottom(),
            rect.center_bottom(),
            rect.right_bottom(),
        ] {
            painter.rect_filled(
                egui::Rect::from_center_size(p, egui::vec2(7., 7.)),
                1,
                egui::Color32::WHITE,
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(p, egui::vec2(7., 7.)),
                1,
                egui::Stroke::new(1., egui::Color32::BLACK),
                egui::StrokeKind::Outside,
            );
        }
        let pan = ui.input(|i| i.key_down(egui::Key::Space));
        let handle_at = |p: egui::Pos2| {
            [
                if (p.x - rect.min.x).abs() < 10. {
                    -1
                } else if (p.x - rect.max.x).abs() < 10. {
                    1
                } else {
                    0
                },
                if (p.y - rect.min.y).abs() < 10. {
                    -1
                } else if (p.y - rect.max.y).abs() < 10. {
                    1
                } else {
                    0
                },
            ]
        };
        if response.hovered()
            && let Some(p) = response.hover_pos()
            && rect.expand(10.).contains(p)
        {
            let handle = s.drag.map_or_else(|| handle_at(p), |(_, _, h)| h);
            let cursor = if pan {
                if response.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                }
            } else {
                match handle {
                    [-1, -1] | [1, 1] => egui::CursorIcon::ResizeNwSe,
                    [-1, 1] | [1, -1] => egui::CursorIcon::ResizeNeSw,
                    [0, -1] | [0, 1] => egui::CursorIcon::ResizeVertical,
                    [-1, 0] | [1, 0] => egui::CursorIcon::ResizeHorizontal,
                    _ if s.move_image => egui::CursorIcon::Grab,
                    _ => egui::CursorIcon::Move,
                }
            };
            ui.ctx().set_cursor_icon(cursor);
        }
        // Capture before egui's drag threshold: a quick move and release can arrive
        // together, after PointerState has already cleared its press origin.
        if (response.is_pointer_button_down_on() || response.contains_pointer())
            && ui.input(|i| i.pointer.primary_pressed())
            && !pan
            && let Some(p) = ui.input(|i| {
                i.pointer.press_origin().or_else(|| {
                    i.events.iter().find_map(|event| {
                        if let egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            ..
                        } = event
                        {
                            Some(*pos)
                        } else {
                            None
                        }
                    })
                })
            })
            && response.rect.contains(p)
            && ui.ctx().layer_id_at(p) == Some(response.layer_id)
        {
            let handle = handle_at(p);
            if rect.expand(10.).contains(p) {
                s.drag = Some((s.rect, p, handle));
                s.drag_center = self.state.transform.center;
            }
        }
        if let Some((start, origin, handle)) = s.drag
            && let Some(p) = ui.input(|i| i.pointer.latest_pos())
        {
            let sign = if s.move_image && handle == [0, 0] {
                -1.
            } else {
                1.
            };
            s.rect = dragged(
                start,
                [
                    (p.x - origin.x) / physical.x * sign,
                    (p.y - origin.y) / physical.y * sign,
                ],
                handle,
                s.ratio.map(|r| r / (size[0] / size[1])),
            );
        }
        if let Some((start, _, [0, 0])) = s.drag
            && s.move_image
        {
            self.state.transform.center = [
                s.drag_center[0] + s.rect[0] - start[0],
                s.drag_center[1] + s.rect[1] - start[1],
            ];
        }
        if ui.input(|i| !i.pointer.primary_down()) {
            s.drag = None;
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use crate::ui::settings_regressions::{app, settle};
    fn wait(app: &mut TrueRenderer, ctx: &egui::Context, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll(ctx);
            if app
                .editing
                .entries
                .get(id)
                .is_some_and(|e| e.loaded.is_some() && !e.loading && !e.pending)
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn crop_session_reopens_expands_cancels_resets_and_persists_history() {
        let (dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let original = std::fs::read(&item.path).unwrap();
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        let mut initial = app.editing.entries[&item.id].draft.clone().unwrap();
        initial.exposure_ev = 0.75;
        initial.process_version = 3;
        initial.advanced = Some(Default::default());
        initial
            .advanced
            .as_mut()
            .unwrap()
            .masks
            .push(Default::default());
        app.apply_edit_draft(&item.id, initial.clone());
        app.commit_edit(&item.id);
        wait(&mut app, &ctx, &item.id);
        let generation = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .generation;
        app.start_crop(&item);
        app.finish_crop(true);
        assert!(app.editing.crop.is_none());
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .generation,
            generation
        );
        app.start_crop(&item);
        app.editing.crop.as_mut().unwrap().rect = [0.25, 0.25, 0.75, 0.75];
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&initial));
        app.finish_crop(true);
        wait(&mut app, &ctx, &item.id);
        app.crop_window(&ctx);
        let cropped = app.editing.entries[&item.id].draft.clone().unwrap();
        assert_eq!(
            cropped.advanced.as_ref().unwrap().geometry.crop,
            [0.25, 0.25, 0.75, 0.75]
        );
        app.start_crop(&item);
        assert_eq!(app.view_source_size(&item.id, [800, 600]), [800, 600]);
        assert_eq!(app.edited_source_size(&item.id, [800, 600]), [400, 300]);
        app.editing.crop.as_mut().unwrap().rect = [0.1, 0.1, 0.9, 0.9];
        app.finish_crop(false);
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&cropped));
        app.start_crop(&item);
        {
            let s = app.editing.crop.as_mut().unwrap();
            s.rect = [0.05, 0.1, 0.85, 0.9];
            s.center();
        }
        // Navigation does not commit/discard the separate crop draft.
        app.command(Command::Select {
            id: app.state.items[1].id.clone(),
            extend: false,
        });
        assert!(app.editing.crop.is_some());
        app.finish_crop(true);
        wait(&mut app, &ctx, &item.id);
        app.crop_window(&ctx);
        let expanded = app.editing.entries[&item.id].draft.clone().unwrap();
        app.step_edit(&item.id, true);
        wait(&mut app, &ctx, &item.id);
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&cropped));
        app.step_edit(&item.id, false);
        wait(&mut app, &ctx, &item.id);
        assert_eq!(
            app.editing.entries[&item.id].draft.as_ref(),
            Some(&expanded)
        );
        app.start_crop(&item);
        app.editing.crop.as_mut().unwrap().rect = FULL;
        app.finish_crop(true);
        wait(&mut app, &ctx, &item.id);
        app.crop_window(&ctx);
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&initial));
        app.step_edit(&item.id, true);
        wait(&mut app, &ctx, &item.id);
        drop(app);
        let catalog = tr_store::Catalog::open(&dir.path().join("data")).unwrap();
        let reopened = catalog.load_edit(&item.id, initial.raw_engine).unwrap();
        assert_eq!(reopened.recipe, expanded);
        assert_eq!(std::fs::read(&item.path).unwrap(), original);
    }
    #[test]
    fn crop_handles_ratios_rotation_and_valid_area_use_source_coordinates() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        for turn in 0..4 {
            let mut recipe = EditRecipe::neutral(tr_core::decoder::RawEngine::Apple);
            recipe.process_version = 3;
            recipe.advanced = Some(Default::default());
            recipe.advanced.as_mut().unwrap().geometry.quarter_turns = turn;
            let size = if turn % 2 == 0 {
                [800, 600]
            } else {
                [600, 800]
            };
            let mut s = Session::new(item.clone(), recipe);
            for ratio in [0.5, 1., 1.5, 16. / 9., 2.] {
                s.rect = FULL;
                s.aspect(ratio, size);
                let normalized = ratio / (size[0] as f32 / size[1] as f32);
                for handle in [
                    [-1, -1],
                    [0, -1],
                    [1, -1],
                    [-1, 0],
                    [1, 0],
                    [-1, 1],
                    [0, 1],
                    [1, 1],
                    [0, 0],
                ] {
                    for delta in [[-2., -2.], [2., 2.], [0.05, -0.07]] {
                        let next = dragged(s.rect, delta, handle, Some(normalized));
                        let mut g = tr_core::editing::geometry::Geometry {
                            crop: next,
                            ..Default::default()
                        };
                        g.validate().unwrap();
                        assert!(
                            ((next[2] - next[0]) / (next[3] - next[1]) - normalized).abs() < 0.0001
                        );
                        g.angle = 15.;
                        s.initial.advanced.as_mut().unwrap().geometry = g;
                        s.rect = FULL;
                        s.constrain([800, 600]);
                        assert!(s.valid(s.rect, [800, 600]));
                        s.rect = FULL;
                        s.aspect(ratio, size);
                    }
                }
            }
        }
    }
    #[test]
    fn repeated_crops_render_from_original_without_progressive_loss() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        let source = tr_core::color::LinearImage::new(
            80,
            60,
            (0..4800)
                .map(|i| [i as f32 / 4800., 0.2, 0.7, 1.])
                .collect(),
        )
        .unwrap();
        let initial = EditRecipe::neutral(tr_core::decoder::RawEngine::Apple);
        let mut recipe = initial.clone();
        for _ in 0..20 {
            let mut s = Session::new(item.clone(), recipe);
            s.rect = [0.25, 0.25, 0.75, 0.75];
            recipe = s.recipe();
            let mut crop = source.clone();
            assert_eq!(
                recipe.apply_preview(&mut crop, [80, 60], 0).unwrap(),
                [40, 30]
            );
            let mut s = Session::new(item.clone(), recipe);
            s.rect = FULL;
            recipe = s.recipe();
            let mut restored = source.clone();
            assert_eq!(
                recipe.apply_preview(&mut restored, [80, 60], 0).unwrap(),
                [80, 60]
            );
            assert_eq!(restored.pixels, source.pixels);
        }
    }
    #[test]
    fn failed_crop_save_retains_recipe_and_session_for_retry() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        app.start_crop(&item);
        app.editing.crop.as_mut().unwrap().rect = [0.1, 0.1, 0.9, 0.9];
        let draft = app.editing.crop.as_ref().unwrap().recipe();
        app.editing
            .entries
            .get_mut(&item.id)
            .unwrap()
            .loaded
            .as_mut()
            .unwrap()
            .generation = 900;
        app.finish_crop(true);
        wait(&mut app, &ctx, &item.id);
        assert!(app.editing.entries[&item.id].error.is_some());
        assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&draft));
        assert!(app.editing.crop.as_ref().unwrap().saving);
    }
    #[test]
    fn escape_from_crop_controls_discards_draft_without_leaving_viewer() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        app.start_crop(&item);
        app.editing.crop.as_mut().unwrap().rect = [0.1, 0.1, 0.8, 0.8];
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("crop-control")));
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| app.keyboard(ui.ctx()),
        );
        output.textures_delta.clear();
        assert!(app.editing.crop.is_none());
        assert_eq!(app.state.view, ViewMode::Preview);
        assert!(!app.editing.entries[&item.id].dirty());
    }
    #[test]
    fn pointer_modes_separate_crop_movement_image_movement_and_view_pan() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        app.start_crop(&item);
        let initial_recipe = app.editing.entries[&item.id].draft.clone();
        let rect = egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(800., 600.));
        for mode in 0..5 {
            let start = [0.2, 0.2, 0.8, 0.8];
            let s = app.editing.crop.as_mut().unwrap();
            s.rect = start;
            s.move_image = mode == 1 || mode == 3;
            app.state.transform = ViewTransform::default();
            let scale =
                ViewTransform::default().scale([800., 600.], [800., 600.], ctx.pixels_per_point());
            let origin = if mode == 3 {
                rect.center() + egui::vec2(800. * scale * 0.3, 0.)
            } else {
                egui::pos2(400., 300.)
            };
            let end = origin + egui::vec2(80., 60.);
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: if mode == 4 {
                    egui::PointerButton::Secondary
                } else {
                    egui::PointerButton::Primary
                },
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let space = |pressed| egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [
                vec![egui::Event::PointerMoved(origin)],
                if mode == 2 {
                    vec![space(true), button(origin, true)]
                } else {
                    vec![button(origin, true)]
                },
                vec![egui::Event::PointerMoved(end)],
                vec![button(end, false), space(false)],
            ] {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1000., 800.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            rect,
                            egui::Id::new("crop-pointer-test"),
                            egui::Sense::click_and_drag(),
                        );
                        app.crop_interaction(ui, &item, &response, [800, 600]);
                    },
                );
                output.textures_delta.clear();
            }
            let actual = app.editing.crop.as_ref().unwrap().rect;
            let sign = match mode {
                0 => 1.,
                1 => -1.,
                _ => 0.,
            };
            let scale =
                ViewTransform::default().scale([800., 600.], [800., 600.], ctx.pixels_per_point());
            for i in 0..4 {
                assert!(
                    (actual[i]
                        - start[i]
                        - if mode == 3 && i == 2 {
                            0.1 / scale
                        } else {
                            sign * 0.1 / scale
                        })
                    .abs()
                        < 0.0001,
                    "mode {mode}: {actual:?}"
                );
            }
            let center = app.state.transform.center;
            let expected_center = if mode == 1 {
                0.5 + actual[0] - start[0]
            } else {
                0.5
            };
            assert!((center[0] - expected_center).abs() < 0.0001);
            assert!((center[1] - expected_center).abs() < 0.0001);
            assert!(app.editing.crop.as_ref().unwrap().drag.is_none());
            assert_eq!(app.editing.entries[&item.id].draft, initial_recipe);
        }
    }
    fn cached_source(app: &mut TrueRenderer, item: &Item) {
        let source = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(80, 60, vec![[0.2, 0.4, 0.6, 1.]; 4800]).unwrap(),
                PreviewRequest::full(),
            )
            .unwrap(),
        );
        app.cache.insert(
            app.image_key(item, 0),
            CachedImage {
                digest: item.digest.clone(),
                info: RasterInfo {
                    shooting: None,
                    scientific: None,
                    reference_mip: None,
                    width: 80,
                    height: 60,
                    source_width: 80,
                    source_height: 60,
                    native_bits: 32,
                    format: "test".into(),
                    decoder: "test".into(),
                    input_color: "linear Rec2020".into(),
                    filter: "reference".into(),
                    orientation: "applied".into(),
                },
                histogram: source.source().histogram(),
                pyramid: source,
                touched: 0,
                transport: "test",
                worker_pid: None,
            },
        );
    }
    fn shape_texts(shape: &egui::Shape, texts: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => texts.push((
                t.galley.job.text.clone(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| shape_texts(s, texts)),
            _ => {}
        }
    }
    #[test]
    fn crop_orientation_and_lock_preserve_framing_without_repeated_shrinkage() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        for size in [[800, 600], [600, 800]] {
            for choice in [
                RatioChoice::Free,
                RatioChoice::Original,
                RatioChoice::Preset(3, 2),
                RatioChoice::Custom,
            ] {
                let mut s = Session::new(
                    item.clone(),
                    EditRecipe::neutral(tr_core::decoder::RawEngine::Apple),
                );
                s.rect = [0.3, 0.3, 0.7, 0.7];
                s.custom = [5., 4.];
                s.choose_ratio(choice, size);
                let start = s.rect;
                let ratio = s.current_ratio(size);
                let area = (start[2] - start[0]) * (start[3] - start[1]);
                for _ in 0..20 {
                    s.invert_orientation(size);
                    assert!((s.current_ratio(size) - 1. / ratio).abs() < 0.0001);
                    assert!(
                        ((s.rect[2] - s.rect[0]) * (s.rect[3] - s.rect[1]) - area).abs() < 0.0001
                    );
                    s.invert_orientation(size);
                }
                for (a, b) in s.rect.into_iter().zip(start) {
                    assert!((a - b).abs() < 0.0001);
                }
                s.choose_ratio(RatioChoice::Free, size);
                assert!(s.ratio.is_none());
                let before_lock = s.rect;
                s.choose_ratio(RatioChoice::Current, size);
                assert_eq!(s.rect, before_lock);
                assert!((s.ratio.unwrap() - ratio).abs() < 0.0001);
                s.reset();
                assert_eq!(s.rect, FULL);
                assert!(s.ratio.is_none());
                assert!(s.recipe().is_neutral());
            }
        }
    }
    #[test]
    fn crop_panel_is_compact_localized_and_only_custom_ratios_show_numbers() {
        for lang in [Language::Italian, Language::English] {
            let (_dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            let item = app.state.items[0].clone();
            app.state.current = Some(item.id.clone());
            app.ensure_edit_loaded(&item);
            wait(&mut app, &ctx, &item.id);
            cached_source(&mut app, &item);
            app.cache_settings.language = lang;
            app.start_crop(&item);
            let initial = app.editing.crop.as_ref().unwrap().recipe();
            for custom in [false, true, false] {
                app.editing.crop.as_mut().unwrap().choose_ratio(
                    if custom {
                        RatioChoice::Custom
                    } else {
                        RatioChoice::Free
                    },
                    [80, 60],
                );
                let mut texts = Vec::new();
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(800., 600.),
                            )),
                            ..Default::default()
                        },
                        |ui| app.crop_window(ui.ctx()),
                    );
                    texts.clear();
                    for shape in &output.shapes {
                        shape_texts(&shape.shape, &mut texts);
                    }
                    output.textures_delta.clear();
                }
                for required in [
                    "Proporzioni",
                    "Griglia dei terzi",
                    "Immagine intera",
                    "Applica ritaglio",
                    "Annulla ritaglio",
                ] {
                    let (_, rect) = texts
                        .iter()
                        .find(|(t, _)| t == lang.text(required))
                        .unwrap_or_else(|| panic!("missing {required}: {texts:?}"));
                    assert!(
                        rect.min.x >= 0. && rect.max.x <= 800. && rect.max.y < 600.,
                        "{required}: {rect:?}"
                    );
                }
                assert_eq!(
                    texts.iter().any(|(t, _)| t == lang.text("Rapporto")),
                    custom
                );
                for removed in [
                    "Bordo sinistro",
                    "Bordo superiore",
                    "Bordo destro",
                    "Bordo inferiore",
                ] {
                    assert!(!texts.iter().any(|(t, _)| t == lang.text(removed)));
                }
                let top = texts
                    .iter()
                    .map(|(_, r)| r.top())
                    .fold(f32::INFINITY, f32::min);
                let bottom = texts.iter().map(|(_, r)| r.bottom()).fold(0., f32::max);
                assert!(
                    bottom - top < if custom { 370. } else { 340. },
                    "panel too tall: {}; {texts:?}",
                    bottom - top
                );
            }
            app.finish_crop(false);
            assert_eq!(app.editing.entries[&item.id].draft.as_ref(), Some(&initial));
        }
    }
    #[test]
    fn crop_keyboard_moves_resizes_and_swaps_without_navigating_or_saving_until_enter() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.state.current = Some(item.id.clone());
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        cached_source(&mut app, &item);
        app.start_crop(&item);
        let initial = app.editing.entries[&item.id].draft.clone();
        app.editing.crop.as_mut().unwrap().rect = [0.2, 0.2, 0.8, 0.8];
        let focus = egui::Id::new("crop-keyboard-photo");
        app.image_focus_ids.insert(focus);
        let press = |app: &mut TrueRenderer, key| {
            for pressed in [true, false] {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        events: vec![egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        ..Default::default()
                    },
                    |ui| {
                        ui.interact(
                            egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(200., 200.)),
                            focus,
                            egui::Sense::click(),
                        )
                        .request_focus();
                        app.keyboard(ui.ctx());
                    },
                );
                out.textures_delta.clear();
            }
        };
        press(&mut app, egui::Key::ArrowRight);
        assert!((app.editing.crop.as_ref().unwrap().rect[0] - 0.2125).abs() < 0.0001);
        assert_eq!(app.state.current.as_deref(), Some(item.id.as_str()));
        press(&mut app, egui::Key::Minus);
        assert!(
            app.editing.crop.as_ref().unwrap().rect[2] - app.editing.crop.as_ref().unwrap().rect[0]
                < 0.6
        );
        let before = app.editing.crop.as_ref().unwrap().current_ratio([80, 60]);
        press(&mut app, egui::Key::X);
        assert!(
            (app.editing.crop.as_ref().unwrap().current_ratio([80, 60]) - 1. / before).abs()
                < 0.0001
        );
        assert_eq!(app.editing.entries[&item.id].draft, initial);
        let expected = app.editing.crop.as_ref().unwrap().recipe();
        press(&mut app, egui::Key::Enter);
        wait(&mut app, &ctx, &item.id);
        app.crop_window(&ctx);
        assert!(app.editing.crop.is_none());
        assert_eq!(
            app.editing.entries[&item.id].draft.as_ref(),
            Some(&expected)
        );
    }
    #[test]
    fn crop_context_menu_opens_from_keyboard_and_keeps_the_photo_menu_reachable() {
        for lang in [Language::Italian, Language::English] {
            let (_dir, ctx, mut app) = app();
            settle(&mut app, &ctx, true);
            let item = app.state.items[0].clone();
            app.state.current = Some(item.id.clone());
            app.ensure_edit_loaded(&item);
            wait(&mut app, &ctx, &item.id);
            cached_source(&mut app, &item);
            app.cache_settings.language = lang;
            app.start_crop(&item);
            let focus = egui::Id::new("crop-menu-photo");
            let mut texts = Vec::new();
            for step in 0..4 {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900., 900.),
                        )),
                        events: if step == 1 {
                            vec![
                                egui::Event::ModifiersChanged(egui::Modifiers::SHIFT),
                                egui::Event::Key {
                                    key: egui::Key::F10,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: egui::Modifiers::SHIFT,
                                },
                            ]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(200., 200.)),
                            focus,
                            egui::Sense::click(),
                        );
                        if step == 0 {
                            response.request_focus();
                        }
                        app.crop_menu_for(&response, &item);
                    },
                );
                texts.clear();
                for shape in &out.shapes {
                    shape_texts(&shape.shape, &mut texts);
                }
                out.textures_delta.clear();
            }
            assert!(egui::Popup::is_any_open(&ctx));
            for label in [
                "Applica ritaglio",
                "Inverti orientamento",
                "Menu fotografia",
            ] {
                assert!(
                    texts.iter().any(|(t, _)| t == lang.text(label)),
                    "{label}: {texts:?}"
                );
            }
            let photo_menu = texts
                .iter()
                .find(|(t, _)| t == lang.text("Menu fotografia"))
                .unwrap()
                .1
                .center();
            for pressed in [true, false, false] {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900., 900.),
                        )),
                        events: vec![
                            egui::Event::PointerMoved(photo_menu),
                            egui::Event::PointerButton {
                                pos: photo_menu,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(200., 200.)),
                            focus,
                            egui::Sense::click(),
                        );
                        app.crop_menu_for(&response, &item);
                    },
                );
                texts.clear();
                for shape in &out.shapes {
                    shape_texts(&shape.shape, &mut texts);
                }
                out.textures_delta.clear();
            }
            for label in [
                "Usa come confronto A",
                "Usa come confronto B",
                "Confronta Prima/Dopo",
            ] {
                assert!(
                    texts.iter().any(|(t, _)| t == lang.text(label)),
                    "{label}: {texts:?}"
                );
            }
            assert_eq!(app.state.current.as_deref(), Some(item.id.as_str()));
            assert!(!app.editing.entries[&item.id].dirty());
        }
    }
    #[test]
    fn crop_fast_drag_keeps_the_press_origin_when_motion_and_release_share_a_frame() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        app.start_crop(&item);
        let viewport = egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(800., 600.));
        let scale =
            ViewTransform::default().scale([800., 600.], [800., 600.], ctx.pixels_per_point());
        for packed in [false, true] {
            app.editing.crop.as_mut().unwrap().rect = [0.2, 0.2, 0.8, 0.8];
            let origin = viewport.center() + egui::vec2(800. * scale * 0.3, 0.);
            let end = origin - egui::vec2(80., 0.);
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let frames = if packed {
                vec![
                    vec![egui::Event::PointerMoved(origin)],
                    vec![
                        button(origin, true),
                        egui::Event::PointerMoved(end),
                        button(end, false),
                    ],
                ]
            } else {
                vec![
                    vec![egui::Event::PointerMoved(origin)],
                    vec![button(origin, true)],
                    vec![egui::Event::PointerMoved(end), button(end, false)],
                ]
            };
            for events in frames {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1000., 800.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            viewport,
                            egui::Id::new("fast-crop"),
                            egui::Sense::click_and_drag(),
                        );
                        app.crop_interaction(ui, &item, &response, [800, 600]);
                    },
                );
                out.textures_delta.clear();
            }
            let actual = app.editing.crop.as_ref().unwrap().rect;
            assert!(
                (actual[2] - (0.8 - 0.1 / scale)).abs() < 0.0001,
                "{actual:?}"
            );
            assert!(app.editing.crop.as_ref().unwrap().drag.is_none());
            assert!(!app.editing.entries[&item.id].dirty());
        }
    }
    #[test]
    fn crop_session_is_suspended_outside_its_preview_and_comparison_uses_saved_recipe() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        let mut saved = app.editing.entries[&item.id].draft.clone().unwrap();
        saved.exposure_ev = 0.75;
        saved.process_version = 3;
        saved
            .advanced
            .get_or_insert_with(Default::default)
            .geometry
            .crop = [0.2, 0.1, 0.7, 0.8];
        app.apply_edit_draft(&item.id, saved.clone());
        app.commit_edit(&item.id);
        wait(&mut app, &ctx, &item.id);
        app.start_crop(&item);
        let draft = [0.1, 0.05, 0.9, 0.95];
        app.editing.crop.as_mut().unwrap().rect = draft;
        assert!(app.crop_active(&item.id));
        assert_eq!(
            app.view_recipe(&item.id)
                .unwrap()
                .advanced
                .unwrap()
                .geometry
                .crop,
            FULL
        );
        let source = Arc::new(
            ImageLevels::from_source(
                tr_core::color::LinearImage::new(80, 60, vec![[0.2, 0.3, 0.4, 1.]; 80 * 60])
                    .unwrap(),
                PreviewRequest::full(),
            )
            .unwrap(),
        );
        let digest = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .source_digest
            .clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app
            .edited_preview(&item.id, &digest, source.clone())
            .is_none()
        {
            app.poll_edit_preview();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            app.progressive_edit_preview(&item.id, source.id())
                .is_some()
        );
        for mode in [ViewMode::Grid, ViewMode::Compare] {
            app.editing.crop.as_mut().unwrap().drag = Some((draft, egui::pos2(400., 300.), [0, 0]));
            app.state.view = mode;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.crop_window(ui.ctx()));
            output.textures_delta.clear();
            assert!(
                !app.crop_active(&item.id),
                "crop should be suspended in {mode:?}"
            );
            assert_eq!(app.view_recipe(&item.id), Some(saved.clone()));
            assert!(
                app.progressive_edit_preview(&item.id, source.id())
                    .is_none()
            );
            assert_eq!(app.editing.crop.as_ref().unwrap().rect, draft);
            assert!(app.editing.crop.as_ref().unwrap().drag.is_none());
        }
        app.state.view = ViewMode::Preview;
        app.state.current = Some(app.state.items[1].id.clone());
        assert!(!app.crop_active(&item.id));
        assert_eq!(app.view_recipe(&item.id), Some(saved.clone()));
        app.state.current = Some(item.id.clone());
        assert!(app.crop_active(&item.id));
        assert!(
            app.progressive_edit_preview(&item.id, source.id())
                .is_some()
        );
        assert_eq!(app.editing.crop.as_ref().unwrap().rect, draft);
        app.finish_crop(false);
        assert_eq!(app.view_recipe(&item.id), Some(saved));
    }
    #[test]
    fn crop_valid_area_accepts_source_edges_without_shrinking_for_float_roundoff() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let mut s = Session::new(
            app.state.items[0].clone(),
            EditRecipe::neutral(app.cache_settings.raw_engine),
        );
        let rect = [0.2, 0.18, 1., 1.];
        for turn in 0..4 {
            for flip in [false, true] {
                let geometry = &mut s
                    .initial
                    .advanced
                    .get_or_insert_with(Default::default)
                    .geometry;
                geometry.quarter_turns = turn;
                geometry.flip_horizontal = flip;
                geometry.flip_vertical = flip;
                s.rect = rect;
                assert!(s.valid(rect, [1200, 800]), "turn={turn} flip={flip}");
                s.constrain([1200, 800]);
                assert_eq!(s.rect, rect);
            }
        }
        s.initial.advanced.as_mut().unwrap().geometry.angle = 12.;
        assert!(!s.valid(FULL, [1200, 800]));
        assert!(s.valid([0.3, 0.3, 0.7, 0.7], [1200, 800]));
    }
    #[test]
    fn crop_click_without_movement_keeps_exact_recipe_and_creates_no_revision() {
        let (_dir, ctx, mut app) = app();
        settle(&mut app, &ctx, true);
        let item = app.state.items[0].clone();
        app.ensure_edit_loaded(&item);
        wait(&mut app, &ctx, &item.id);
        let mut recipe = app.editing.entries[&item.id].draft.clone().unwrap();
        recipe.process_version = 3;
        recipe
            .advanced
            .get_or_insert_with(Default::default)
            .geometry
            .crop = [0.137, 0.169, 0.681, 0.827];
        app.apply_edit_draft(&item.id, recipe.clone());
        app.commit_edit(&item.id);
        wait(&mut app, &ctx, &item.id);
        let generation = app.editing.entries[&item.id]
            .loaded
            .as_ref()
            .unwrap()
            .generation;
        app.start_crop(&item);
        let viewport = egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(800., 600.));
        let p = egui::pos2(400., 300.);
        for events in [
            vec![egui::Event::PointerMoved(p)],
            vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(
                        viewport,
                        egui::Id::new("no-op-crop"),
                        egui::Sense::click_and_drag(),
                    );
                    app.crop_interaction(ui, &item, &response, [800, 600]);
                },
            );
            out.textures_delta.clear();
        }
        assert_eq!(app.editing.crop.as_ref().unwrap().recipe(), recipe);
        app.finish_crop(true);
        assert!(app.editing.crop.is_none());
        assert_eq!(
            app.editing.entries[&item.id]
                .loaded
                .as_ref()
                .unwrap()
                .generation,
            generation
        );
    }
}
