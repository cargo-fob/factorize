use std::collections::HashSet;
use std::path::PathBuf;

use crate::ignored::FsWatcherIgnored;

/// 경로의 상태를 추적하는 단위
/// rspack의 PathTracker에 대응
#[derive(Debug, Default)]
struct PathTracker {
    /// 현재 감시 중인 전체 경로
    all: HashSet<PathBuf>,
    /// 마지막 update에서 추가된 경로
    added: HashSet<PathBuf>,
    /// 마지막 update에서 제거된 경로
    removed: HashSet<PathBuf>,
}

impl PathTracker {
    fn reset(&mut self) {
        self.added.clear();
        self.removed.clear();
    }

    fn add(&mut self, path: PathBuf) {
        self.added.insert(path.clone());
        self.all.insert(path);
    }

    fn remove(&mut self, path: PathBuf) {
        self.all.remove(&path);
        self.removed.insert(path);
    }

    fn update(
        &mut self,
        added: impl Iterator<Item = PathBuf>,
        removed: impl Iterator<Item = PathBuf>,
        ignored: &FsWatcherIgnored,
    ) {
        for path in added {
            if ignored.should_ignore(&path.to_string_lossy()) {
                continue;
            }
            // 상대 경로를 절대 경로로 변환
            let abs = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .unwrap_or_default()
                    .join(path)
            };
            self.add(abs);
        }

        for path in removed {
            let abs = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .unwrap_or_default()
                    .join(path)
            };
            self.remove(abs);
        }
    }
}

/// 경로 읽기 전용 접근자
/// rspack의 PathAccessor에 대응
pub struct PathAccessor<'a> {
    files: &'a PathTracker,
    directories: &'a PathTracker,
    missing: &'a PathTracker,
}

impl<'a> PathAccessor<'a> {
    /// (all, added, removed)
    pub fn files(&self) -> (&HashSet<PathBuf>, &HashSet<PathBuf>, &HashSet<PathBuf>) {
        (&self.files.all, &self.files.added, &self.files.removed)
    }

    pub fn directories(&self) -> (&HashSet<PathBuf>, &HashSet<PathBuf>, &HashSet<PathBuf>) {
        (
            &self.directories.all,
            &self.directories.added,
            &self.directories.removed,
        )
    }

    pub fn missing(&self) -> (&HashSet<PathBuf>, &HashSet<PathBuf>, &HashSet<PathBuf>) {
        (&self.missing.all, &self.missing.added, &self.missing.removed)
    }

    /// 모든 경로를 하나의 이터레이터로 반환
    pub fn all(&self) -> impl Iterator<Item = &PathBuf> {
        self.files
            .all
            .iter()
            .chain(self.directories.all.iter())
            .chain(self.missing.all.iter())
    }
}

/// 파일/디렉토리/missing 경로를 분리 관리
/// rspack의 PathManager에 대응
#[derive(Default)]
pub struct PathManager {
    files: PathTracker,
    directories: PathTracker,
    missing: PathTracker,
    ignored: FsWatcherIgnored,
}

impl PathManager {
    pub fn new(ignored: FsWatcherIgnored) -> Self {
        Self {
            files: PathTracker::default(),
            directories: PathTracker::default(),
            missing: PathTracker::default(),
            ignored,
        }
    }

    /// added/removed 초기화 (watch 호출마다 리셋)
    pub fn reset(&mut self) {
        self.files.reset();
        self.directories.reset();
        self.missing.reset();
    }

    /// 경로 업데이트: files, directories, missing 각각 (added, removed) 튜플
    pub fn update(
        &mut self,
        files: (impl Iterator<Item = PathBuf>, impl Iterator<Item = PathBuf>),
        directories: (impl Iterator<Item = PathBuf>, impl Iterator<Item = PathBuf>),
        missing: (impl Iterator<Item = PathBuf>, impl Iterator<Item = PathBuf>),
    ) {
        self.files.update(files.0, files.1, &self.ignored);
        self.directories
            .update(directories.0, directories.1, &self.ignored);
        self.missing.update(missing.0, missing.1, &self.ignored);
    }

    /// 읽기 전용 접근자
    pub fn access(&self) -> PathAccessor<'_> {
        PathAccessor {
            files: &self.files,
            directories: &self.directories,
            missing: &self.missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_tracker_add_remove() {
        let mut tracker = PathTracker::default();
        let path = PathBuf::from("/src/index.ts");

        tracker.add(path.clone());
        assert!(tracker.all.contains(&path));
        assert!(tracker.added.contains(&path));

        tracker.remove(path.clone());
        assert!(!tracker.all.contains(&path));
        assert!(tracker.removed.contains(&path));
    }

    #[test]
    fn test_path_manager_update() {
        let ignored = FsWatcherIgnored::Path("**/node_modules/**".to_string());
        let mut manager = PathManager::new(ignored);

        manager.update(
            (
                vec![PathBuf::from("/src/index.ts")].into_iter(),
                vec![].into_iter(),
            ),
            (
                vec![
                    PathBuf::from("/src"),
                    PathBuf::from("/project/node_modules/lodash"),
                ]
                .into_iter(),
                vec![].into_iter(),
            ),
            (
                vec![PathBuf::from("/src/missing.ts")].into_iter(),
                vec![].into_iter(),
            ),
        );

        let accessor = manager.access();

        // files
        assert!(accessor.files().0.contains(&PathBuf::from("/src/index.ts")));

        // directories — node_modules 하위는 ignored
        assert!(accessor.directories().0.contains(&PathBuf::from("/src")));
        assert!(!accessor
            .directories()
            .0
            .contains(&PathBuf::from("/project/node_modules/lodash")));

        // missing
        assert!(accessor
            .missing()
            .0
            .contains(&PathBuf::from("/src/missing.ts")));

        // all: 3개 (node_modules 제외)
        assert_eq!(accessor.all().count(), 3);
    }

    #[test]
    fn test_path_manager_reset() {
        let mut manager = PathManager::default();

        manager.update(
            (
                vec![PathBuf::from("/src/index.ts")].into_iter(),
                vec![].into_iter(),
            ),
            (vec![].into_iter(), vec![].into_iter()),
            (vec![].into_iter(), vec![].into_iter()),
        );

        assert_eq!(manager.access().files().1.len(), 1); // added = 1

        manager.reset();
        assert_eq!(manager.access().files().1.len(), 0); // added = 0
        assert_eq!(manager.access().files().0.len(), 1); // all은 유지
    }
}
