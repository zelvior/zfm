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
