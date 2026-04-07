use std::collections::HashSet;
use std::path::PathBuf;

use tokio::sync::mpsc::UnboundedSender;

use crate::paths::PathManager;
use crate::{EventBatch, FsEvent, FsEventKind};

/// 파일 시스템 이벤트를 받아 관련 의존성을 찾고 이벤트를 전파하는 트리거
/// rspack의 Trigger + DependencyFinder에 대응
///
/// /src/index.ts 변경 시:
///   - /src/index.ts 자체 (파일로 등록됐으면) → Change
///   - /src          (디렉토리로 등록됐으면)   → Change
///   - /             (디렉토리로 등록됐으면)   → Change
pub struct Trigger {
    tx: UnboundedSender<EventBatch>,
}

impl Trigger {
    pub fn new(tx: UnboundedSender<EventBatch>) -> Self {
        Self { tx }
    }

    /// 이벤트 발생 시 호출. 관련 경로를 찾아 이벤트 전파
    pub fn on_event(&self, path: &PathBuf, kind: FsEventKind, path_manager: &PathManager) {
        let events = self.find_associated_events(path, kind, path_manager);
        if !events.is_empty() {
            let _ = self.tx.send(events);
        }
    }

    /// 주어진 경로와 관련된 모든 이벤트를 찾음
    fn find_associated_events(
        &self,
        path: &PathBuf,
        kind: FsEventKind,
        path_manager: &PathManager,
    ) -> EventBatch {
        let accessor = path_manager.access();
        let (files, _, _) = accessor.files();
        let (directories, _, _) = accessor.directories();
        let (missing, _, _) = accessor.missing();

        let mut events = Vec::new();

        if path.exists() {
            // 파일이면서 등록된 파일 → 이벤트 추가
            if path.is_file() && (files.contains(path) || missing.contains(path)) {
                events.push(FsEvent {
                    path: path.clone(),
                    kind,
                });
            }

            // 디렉토리이면서 등록된 디렉토리 → 이벤트 추가
            if path.is_dir() && (directories.contains(path) || missing.contains(path)) {
                events.push(FsEvent {
                    path: path.clone(),
                    kind,
                });
            }
        } else if files.contains(path) || directories.contains(path) || missing.contains(path) {
            // 존재하지 않지만 등록된 경로 → 이벤트 추가
            events.push(FsEvent {
                path: path.clone(),
                kind,
            });
        }

        // 부모 디렉토리 재귀 탐색
        self.find_parent_directories(path, directories, &mut events);

        events
    }

    /// 등록된 부모 디렉토리를 찾아 Change 이벤트 추가
    fn find_parent_directories(
        &self,
        path: &PathBuf,
        directories: &HashSet<PathBuf>,
        events: &mut Vec<FsEvent>,
    ) {
        let mut current = path.as_path();
        while let Some(parent) = current.parent() {
            let parent_buf = parent.to_path_buf();
            if directories.contains(&parent_buf) {
                // 부모 디렉토리는 항상 Change (자식이 뭘 하든 디렉토리 자체는 "변경됨")
                events.push(FsEvent {
                    path: parent_buf,
                    kind: FsEventKind::Change,
                });
            }
            current = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::PathManager;

    #[test]
    fn test_find_file_event() {
        let mut path_manager = PathManager::default();
        path_manager.update(
            (
                vec![PathBuf::from("/path/a/b/index.js")].into_iter(),
                vec![].into_iter(),
            ),
            (
                vec![PathBuf::from("/path/a/b")].into_iter(),
                vec![].into_iter(),
            ),
            (vec![].into_iter(), vec![].into_iter()),
        );

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let trigger = Trigger::new(tx);

        // 존재하지 않는 파일이지만 등록됨 → 이벤트 발생
        let events = trigger.find_associated_events(
            &PathBuf::from("/path/a/b/index.js"),
            FsEventKind::Remove,
            &path_manager,
        );

        // /path/a/b/index.js (Remove) + /path/a/b (Change)
        assert!(events.len() >= 2);
        assert!(events.contains(&FsEvent {
            path: PathBuf::from("/path/a/b/index.js"),
            kind: FsEventKind::Remove,
        }));
        assert!(events.contains(&FsEvent {
            path: PathBuf::from("/path/a/b"),
            kind: FsEventKind::Change,
        }));
    }

    #[test]
    fn test_parent_directory_notification() {
        let mut path_manager = PathManager::default();
        path_manager.update(
            (vec![].into_iter(), vec![].into_iter()),
            (
                vec![
                    PathBuf::from("/path/a/b/c"),
                    PathBuf::from("/path/a/b"),
                ]
                .into_iter(),
                vec![].into_iter(),
            ),
            (vec![].into_iter(), vec![].into_iter()),
        );

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let trigger = Trigger::new(tx);

        // /path/a/b/c/index.js 변경 → 부모 디렉토리들에 Change 이벤트
        let events = trigger.find_associated_events(
            &PathBuf::from("/path/a/b/c/index.js"),
            FsEventKind::Create,
            &path_manager,
        );

        assert!(events.contains(&FsEvent {
            path: PathBuf::from("/path/a/b/c"),
            kind: FsEventKind::Change,
        }));
        assert!(events.contains(&FsEvent {
            path: PathBuf::from("/path/a/b"),
            kind: FsEventKind::Change,
        }));
    }
}
