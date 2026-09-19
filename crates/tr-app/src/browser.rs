//! Filesystem presentation state; no filesystem, catalog or image work here.
use std::{cmp::Ordering, collections::HashMap, path::PathBuf, sync::Arc};
use tr_core::budget::Lease;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Directory,
    Image,
    File,
    Link,
    Package,
    Special,
}
#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    /// Credits follow entries through messages, listings and visible rows.
    pub credit: Option<Arc<Lease>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListingState {
    Unloaded,
    Loading,
    Ready,
    Partial(String),
    Failed(String),
}
pub struct Branch {
    pub epoch: u64,
    pub expanded: bool,
    pub entries: Vec<Arc<Entry>>,
    pub state: ListingState,
    pub touched: u64,
}
#[derive(Clone)]
pub struct Row {
    pub entry: Arc<Entry>,
    pub depth: usize,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum EntryFilter {
    #[default]
    All,
    Images,
    Directories,
}
#[derive(Default)]
pub struct Browser {
    pub roots: Vec<Arc<Entry>>,
    pub branches: HashMap<PathBuf, Branch>,
    pub rows: Vec<Row>,
    pub focused: Option<PathBuf>,
    pub selected: Option<PathBuf>,
    pub filter: String,
    pub entry_filter: EntryFilter,
    pub show_hidden: bool,
    pub revision: u64,
}
impl Browser {
    pub fn add_root(&mut self, path: PathBuf) {
        if self.roots.iter().any(|e| e.path == path) || self.roots.len() >= 128 {
            return;
        }
        self.roots.push(Arc::new(Entry {
            name: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into(),
            path,
            kind: Kind::Directory,
            credit: None,
        }));
        self.rebuild();
    }
    pub fn begin(&mut self, path: PathBuf) -> u64 {
        self.revision += 1;
        let branch = self.branches.entry(path).or_insert(Branch {
            epoch: 0,
            expanded: true,
            entries: vec![],
            state: ListingState::Unloaded,
            touched: 0,
        });
        branch.epoch = self.revision;
        branch.expanded = true;
        branch.touched = self.revision;
        branch.state = ListingState::Loading;
        self.rebuild();
        self.revision
    }
    pub fn collapse(&mut self, path: &PathBuf) {
        if let Some(branch) = self.branches.get_mut(path) {
            branch.expanded = false;
            branch.epoch += 1;
            if branch.state == ListingState::Loading {
                branch.state = ListingState::Unloaded;
            }
        }
        if self
            .focused
            .as_ref()
            .is_some_and(|focus| focus != path && focus.starts_with(path))
        {
            self.focused = Some(path.clone());
        }
        self.rebuild();
    }
    pub fn accept(
        &mut self,
        path: &PathBuf,
        epoch: u64,
        entries: Vec<Arc<Entry>>,
        result: Option<Result<(), String>>,
    ) -> bool {
        let Some(branch) = self.branches.get_mut(path) else {
            return false;
        };
        if !branch.expanded || branch.epoch != epoch {
            return false;
        }
        // Finished listings arrive already naturally sorted by the I/O worker.
        if let Some(result) = result {
            match result {
                Ok(()) => {
                    branch.entries = entries;
                    branch.state = ListingState::Ready;
                }
                Err(error) => {
                    // Failed/partial refresh cannot prove that old entries disappeared.
                    let mut known: std::collections::HashSet<_> =
                        branch.entries.iter().map(|e| e.path.clone()).collect();
                    branch
                        .entries
                        .extend(entries.into_iter().filter(|e| known.insert(e.path.clone())));
                    branch.state = if branch.entries.is_empty() {
                        ListingState::Failed(error)
                    } else {
                        ListingState::Partial(error)
                    };
                }
            }
        } else {
            let known: std::collections::HashSet<_> =
                branch.entries.iter().map(|e| e.path.clone()).collect();
            branch
                .entries
                .extend(entries.into_iter().filter(|e| !known.contains(&e.path)));
        }
        self.rebuild();
        true
    }
    pub fn evict_collapsed(&mut self) {
        self.branches.retain(|path, b| {
            b.expanded || self.focused.as_ref().is_some_and(|f| f.starts_with(path))
        });
        self.rebuild();
    }
    pub fn rebuild(&mut self) {
        let query = self.filter.to_lowercase();
        let mut rows = Vec::new();
        fn visit(
            browser: &Browser,
            entry: &Arc<Entry>,
            depth: usize,
            query: &str,
            rows: &mut Vec<Row>,
        ) -> bool {
            if depth > 128 {
                return false;
            }
            let start = rows.len();
            rows.push(Row {
                entry: entry.clone(),
                depth,
            });
            if let Some(branch) = browser.branches.get(&entry.path).filter(|b| b.expanded) {
                for child in &branch.entries {
                    visit(browser, child, depth + 1, query, rows);
                }
            }
            let matches_kind = entry.kind == Kind::Directory
                || match browser.entry_filter {
                    EntryFilter::All => true,
                    EntryFilter::Images => entry.kind == Kind::Image,
                    EntryFilter::Directories => false,
                };
            if rows.len() == start + 1
                && depth != 0
                && (!matches_kind || !entry.name.to_lowercase().contains(query))
            {
                rows.pop();
                return false;
            }
            true
        }
        for root in &self.roots {
            visit(self, root, 0, &query, &mut rows);
        }
        self.rows = rows;
    }
}

/// Natural ASCII numeric runs, Unicode case-insensitive display ordering,
/// with the caller providing a lossless native-name tie break.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a = a.to_lowercase();
    let b = b.to_lowercase();
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (start_i, start_j) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let mut x = &a[start_i..i];
            let mut y = &b[start_j..j];
            while x.len() > 1 && x[0] == b'0' {
                x = &x[1..];
            }
            while y.len() > 1 && y[0] == b'0' {
                y = &y[1..];
            }
            let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
            if order != Ordering::Equal {
                return order;
            }
        } else {
            let order = a[i].cmp(&b[j]);
            if order != Ordering::Equal {
                return order;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epochs_are_per_branch_and_collapse_rejects_old_results() {
        let mut b = Browser::default();
        let a = PathBuf::from("/a");
        let c = PathBuf::from("/c");
        b.add_root(a.clone());
        b.add_root(c.clone());
        let a1 = b.begin(a.clone());
        let c1 = b.begin(c.clone());
        b.collapse(&a);
        let a2 = b.begin(a.clone());
        assert!(!b.accept(&a, a1, vec![], Some(Ok(()))));
        assert!(b.accept(&c, c1, vec![], Some(Ok(()))));
        assert!(b.accept(&a, a2, vec![], Some(Ok(()))));
    }
    #[test]
    fn numbers_do_not_overflow_and_sort_naturally() {
        assert_eq!(natural_cmp("IMG2.nef", "img10.nef"), Ordering::Less);
        assert_eq!(natural_cmp("0002", "2"), Ordering::Equal);
        assert_eq!(
            natural_cmp("999999999999999999999", "1000000000000000000000"),
            Ordering::Less
        );
    }
}
