//! Deterministic UI state. Persistent edits only become visible after Commit.
pub mod scheduler;
pub mod budget {
    pub use tr_core::budget::*;
}
use std::collections::BTreeSet;
use tr_core::{Annotation, Item, Label, ViewMode, ViewTransform};

#[derive(Default)]
pub struct State {
    pub items: Vec<Item>,
    pub visible: Vec<usize>,
    pub selected: BTreeSet<String>,
    pub current: Option<String>,
    pub view: ViewMode,
    pub transform: ViewTransform,
    pub query: String,
    pub minimum_rating: i8,
    pub rejected_only: bool,
    pub label_filter: Option<Label>,
    pub pending: BTreeSet<String>,
}
pub enum Command {
    Select {
        id: String,
        extend: bool,
    },
    Move(i32),
    SetView(ViewMode),
    Rate(i8),
    Label(Label),
    Keywords(String),
    Undo,
    Commit {
        id: String,
        annotation: Annotation,
        revision: u64,
    },
    Failed(String),
}
#[derive(Debug)]
pub enum Effect {
    Save {
        id: String,
        expected_revision: u64,
        annotation: Annotation,
    },
    Undo,
}
impl State {
    pub fn replace_items(&mut self, items: Vec<Item>) {
        self.items = items;
        self.selected.clear();
        self.current = None;
        self.pending.clear();
        self.transform = ViewTransform::default();
        self.refilter();
        if let Some(&i) = self.visible.first() {
            let id = self.items[i].id.clone();
            self.selected.insert(id.clone());
            self.current = Some(id);
        }
    }
    pub fn refilter(&mut self) {
        let query = self.query.to_lowercase();
        self.visible = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                let matches_text = query.is_empty()
                    || item.name.to_lowercase().contains(&query)
                    || item
                        .annotation
                        .keywords
                        .iter()
                        .any(|k| k.to_lowercase().contains(&query));
                matches_text
                    && (if self.rejected_only {
                        item.annotation.rating == -1
                    } else {
                        self.minimum_rating == 0 || item.annotation.rating >= self.minimum_rating
                    })
                    && self.label_filter.is_none_or(|l| l == item.annotation.label)
            })
            .map(|(i, _)| i)
            .collect();
        let allowed: BTreeSet<_> = self
            .visible
            .iter()
            .map(|i| self.items[*i].id.clone())
            .collect();
        self.selected.retain(|id| allowed.contains(id));
        if self
            .current
            .as_ref()
            .is_some_and(|id| !allowed.contains(id))
        {
            self.current = self.visible.first().map(|i| self.items[*i].id.clone());
        }
        if self.selected.is_empty()
            && let Some(id) = &self.current
        {
            self.selected.insert(id.clone());
        }
    }
    pub fn current_item(&self) -> Option<&Item> {
        self.current
            .as_ref()
            .and_then(|id| self.items.iter().find(|i| &i.id == id))
    }
    pub fn dispatch(&mut self, command: Command) -> Vec<Effect> {
        match command {
            Command::Select { id, extend } => {
                if !self.visible.iter().any(|i| self.items[*i].id == id) {
                    return vec![];
                }
                if !extend {
                    self.selected.clear();
                }
                if !extend || !self.selected.remove(&id) {
                    self.selected.insert(id.clone());
                }
                self.current = Some(id);
                self.transform = ViewTransform::default();
            }
            Command::Move(delta) => {
                if self.visible.is_empty() {
                    return vec![];
                }
                let pos = self
                    .visible
                    .iter()
                    .position(|i| Some(&self.items[*i].id) == self.current.as_ref())
                    .unwrap_or(0);
                let next =
                    (pos as i64 + delta as i64).clamp(0, self.visible.len() as i64 - 1) as usize;
                let id = self.items[self.visible[next]].id.clone();
                return self.dispatch(Command::Select { id, extend: false });
            }
            Command::SetView(view) => self.view = view,
            Command::Commit {
                id,
                annotation,
                revision,
            } => {
                self.pending.remove(&id);
                if let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                    && revision > item.revision
                {
                    item.annotation = annotation;
                    item.revision = revision;
                }
                self.refilter();
            }
            Command::Failed(id) => {
                self.pending.remove(&id);
            }
            Command::Undo => {
                if self.pending.is_empty() {
                    return vec![Effect::Undo];
                }
            }
            mutation => {
                let mut effects = vec![];
                for item in &self.items {
                    if !self.selected.contains(&item.id) || self.pending.contains(&item.id) {
                        continue;
                    }
                    let mut annotation = item.annotation.clone();
                    match &mutation {
                        Command::Rate(rating) => annotation.rating = *rating,
                        Command::Label(label) => annotation.label = *label,
                        Command::Keywords(text) => {
                            if annotation.set_keywords(text).is_err() {
                                continue;
                            }
                        }
                        _ => unreachable!(),
                    }
                    if annotation != item.annotation && annotation.validate().is_ok() {
                        self.pending.insert(item.id.clone());
                        effects.push(Effect::Save {
                            id: item.id.clone(),
                            expected_revision: item.revision,
                            annotation,
                        });
                    }
                }
                return effects;
            }
        }
        vec![]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn item() -> Item {
        Item {
            id: "a".into(),
            name: "test.png".into(),
            path: "test.png".into(),
            digest: "x".into(),
            bytes: 20,
            approved: true,
            annotation: Annotation::default(),
            revision: 0,
        }
    }
    #[test]
    fn ratings_wait_for_commit_and_deduplicate_pending() {
        let mut state = State::default();
        state.replace_items(vec![item()]);
        assert_eq!(state.dispatch(Command::Rate(5)).len(), 1);
        assert_eq!(state.items[0].annotation.rating, 0);
        assert!(state.dispatch(Command::Rate(3)).is_empty());
        state.dispatch(Command::Commit {
            id: "a".into(),
            annotation: Annotation {
                rating: 5,
                ..Default::default()
            },
            revision: 1,
        });
        assert_eq!(state.items[0].annotation.rating, 5);
    }
    #[test]
    fn filter_does_not_leave_hidden_selection() {
        let mut state = State::default();
        state.replace_items(vec![item()]);
        state.query = "assente".into();
        state.refilter();
        assert!(state.selected.is_empty());
        assert!(state.current.is_none());
        assert!(state.dispatch(Command::Rate(5)).is_empty());
    }
}
