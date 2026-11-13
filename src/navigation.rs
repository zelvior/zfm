use std::path::{Path, PathBuf};

/// History is changed only after the destination has been successfully read.
#[derive(Debug)]
pub struct History {
    paths: Vec<PathBuf>,
    current: usize,
}

#[derive(Clone, Debug)]
pub enum Destination {
    Visit(PathBuf),
    Back,
    Forward,
    Refresh,
}

impl History {
    pub fn new(path: PathBuf) -> Self {
        Self {
            paths: vec![path],
            current: 0,
        }
    }
    pub fn current(&self) -> &Path {
        &self.paths[self.current]
    }
    pub fn resolve(&self, destination: &Destination) -> Option<PathBuf> {
        match destination {
            Destination::Visit(path) => Some(path.clone()),
            Destination::Back => self.current.checked_sub(1).map(|i| self.paths[i].clone()),
            Destination::Forward => self.paths.get(self.current + 1).cloned(),
            Destination::Refresh => Some(self.current().to_path_buf()),
        }
    }
    pub fn commit(&mut self, destination: Destination) {
        match destination {
            Destination::Visit(path) => {
                if path != self.current() {
                    self.paths.truncate(self.current + 1);
                    self.paths.push(path);
                    self.current += 1;
                }
            }
            Destination::Back if self.current > 0 => self.current -= 1,
            Destination::Forward if self.current + 1 < self.paths.len() => self.current += 1,
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_truncates_forward_only_when_visit_commits() {
        let mut h = History::new("/a".into());
        h.commit(Destination::Visit("/b".into()));
        h.commit(Destination::Back);
        assert_eq!(h.resolve(&Destination::Forward), Some("/b".into()));
        // A failed visit never calls commit.
        assert_eq!(h.current(), Path::new("/a"));
        h.commit(Destination::Visit("/c".into()));
        assert_eq!(h.resolve(&Destination::Forward), None);
    }
}

/// Each tab owns independent navigation history; at least one tab is retained.
#[derive(Debug)]
pub struct Tabs {
    histories: Vec<History>,
    active: usize,
}
impl Tabs {
    pub fn new(path: PathBuf) -> Self {
        Self {
            histories: vec![History::new(path)],
            active: 0,
        }
    }
    pub fn current(&self) -> &History {
        &self.histories[self.active]
    }
    pub fn current_mut(&mut self) -> &mut History {
        &mut self.histories[self.active]
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn len(&self) -> usize {
        self.histories.len()
    }
    pub fn is_empty(&self) -> bool {
        self.histories.is_empty()
    }
    pub fn history(&self, index: usize) -> Option<&History> {
        self.histories.get(index)
    }
    pub fn add(&mut self, path: PathBuf) {
        self.histories.push(History::new(path));
        self.active = self.histories.len() - 1;
    }
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.histories.len() || index == self.active {
            false
        } else {
            self.active = index;
            true
        }
    }
    pub fn close(&mut self) -> bool {
        let active = self.active;
        self.close_at(active)
    }
    pub fn close_at(&mut self, index: usize) -> bool {
        if self.histories.len() <= 1 || index >= self.histories.len() {
            return false;
        }
        self.histories.remove(index);
        if index < self.active {
            self.active -= 1;
        } else if self.active >= self.histories.len() {
            self.active = self.histories.len() - 1;
        }
        true
    }
    pub fn close_others(&mut self, keep_index: usize) -> bool {
        if self.histories.len() <= 1 || keep_index >= self.histories.len() {
            return false;
        }
        let kept = self.histories.swap_remove(keep_index);
        self.histories.clear();
        self.histories.push(kept);
        self.active = 0;
        true
    }
    pub fn next_tab(&mut self) -> bool {
        if self.histories.len() <= 1 {
            return false;
        }
        self.active = (self.active + 1) % self.histories.len();
        true
    }
    pub fn prev_tab(&mut self) -> bool {
        if self.histories.len() <= 1 {
            return false;
        }
        self.active = if self.active == 0 {
            self.histories.len() - 1
        } else {
            self.active - 1
        };
        true
    }
}
#[cfg(test)]
mod tab_tests {
    use super::*;
    #[test]
    fn tabs_keep_independent_history_and_at_least_one_tab() {
        let mut tabs = Tabs::new("/a".into());
        tabs.current_mut().commit(Destination::Visit("/b".into()));
        tabs.add("/c".into());
        tabs.current_mut().commit(Destination::Visit("/d".into()));
        assert!(tabs.select(0));
        assert_eq!(tabs.current().current(), Path::new("/b"));
        tabs.current_mut().commit(Destination::Back);
        assert!(tabs.select(1));
        assert_eq!(tabs.current().current(), Path::new("/d"));
        assert!(tabs.close());
        assert_eq!(tabs.current().current(), Path::new("/a"));
        assert!(!tabs.close());
        assert!(!tabs.select(3));
    }
    #[test]
    fn tabs_cycle_close_at_and_close_others() {
        let mut tabs = Tabs::new("/1".into());
        assert!(!tabs.next_tab());
        assert!(!tabs.prev_tab());
        tabs.add("/2".into());
        tabs.add("/3".into());
        assert_eq!(tabs.active(), 2);
        assert!(tabs.next_tab());
        assert_eq!(tabs.active(), 0);
        assert!(tabs.prev_tab());
        assert_eq!(tabs.active(), 2);
        // Closing tab 0 shifts active index from 2 down to 1 ("/3").
        assert!(tabs.close_at(0));
        assert_eq!(tabs.active(), 1);
        assert_eq!(tabs.current().current(), Path::new("/3"));
        tabs.add("/4".into());
        assert!(tabs.close_others(0));
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.current().current(), Path::new("/2"));
        assert!(!tabs.close_others(0));
    }
}
