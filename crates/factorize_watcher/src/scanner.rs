use std::path::PathBuf;
use std::time::SystemTime;

use tokio::sync::mpsc::UnboundedSender;

use crate::paths::PathManager;
use crate::{EventBatch, FsEvent, FsEventKind};

/// watcher 시작 시 이미 변경/삭제된 파일을 감지하는 스캐너
/// rspack의 Scanner에 대응
///
/// 문제: watcher가 attach되기 전에 파일이 변경되면 놓침
/// 해결: start_time 이후 mtime이 바뀐 파일 → Change 이벤트
///       존재하지 않는 파일 → Remove 이벤트
pub struct Scanner {
    tx: Option<UnboundedSender<EventBatch>>,
}

impl Scanner {
    pub fn new(tx: UnboundedSender<EventBatch>) -> Self {
        Self { tx: Some(tx) }
    }

    /// 등록된 경로들을 스캔하여 start_time 이후 변경/삭제 감지
    pub fn scan(&self, path_manager: &PathManager, start_time: SystemTime) {
        let Some(tx) = &self.tx else { return };

        let accessor = path_manager.access();

        // added된 파일들만 스캔 (이미 있던 파일은 스킵)
        let added_files: Vec<PathBuf> = accessor.files().1.iter().cloned().collect();
        let added_dirs: Vec<PathBuf> = accessor.directories().1.iter().cloned().collect();
        let missing_all: Vec<PathBuf> = accessor.missing().0.iter().cloned().collect();

        // 파일 스캔
        Self::scan_missing(&added_files, &missing_all, tx);
        Self::scan_changed(&added_files, &start_time, tx);

        // 디렉토리 스캔
        Self::scan_missing(&added_dirs, &missing_all, tx);
        Self::scan_changed(&added_dirs, &start_time, tx);
    }

    /// 존재하지 않는 경로 → Remove 이벤트
    fn scan_missing(
        paths: &[PathBuf],
        missing: &[PathBuf],
        tx: &UnboundedSender<EventBatch>,
    ) {
        let events: EventBatch = paths
            .iter()
            .filter(|path| !path.exists() && !missing.contains(path))
            .map(|path| FsEvent {
                path: path.clone(),
                kind: FsEventKind::Remove,
            })
            .collect();

        if !events.is_empty() {
            let _ = tx.send(events);
        }
    }

    /// start_time 이후 변경된 경로 → Change 이벤트
    fn scan_changed(
        paths: &[PathBuf],
        start_time: &SystemTime,
        tx: &UnboundedSender<EventBatch>,
    ) {
        let events: EventBatch = paths
            .iter()
            .filter(|path| {
                path.metadata()
                    .and_then(|m| m.modified().or_else(|_| m.created()))
                    .map(|mtime| *start_time < mtime)
                    .unwrap_or(false)
            })
            .map(|path| FsEvent {
                path: path.clone(),
                kind: FsEventKind::Change,
            })
            .collect();

        if !events.is_empty() {
            let _ = tx.send(events);
        }
    }

    pub fn close(&mut self) {
        self.tx.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scan_detects_missing_files() {
        let mut path_manager = PathManager::default();

        // 존재하지 않는 파일을 files로 등록
        path_manager.update(
            (
                vec![PathBuf::from("/nonexistent/file.txt")].into_iter(),
                vec![].into_iter(),
            ),
            (vec![].into_iter(), vec![].into_iter()),
            (vec![].into_iter(), vec![].into_iter()),
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut scanner = Scanner::new(tx);

        scanner.scan(&path_manager, SystemTime::now());
        scanner.close(); // tx drop → rx 끊김

        let mut events = Vec::new();
        while let Some(batch) = rx.recv().await {
            events.extend(batch);
        }

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, FsEventKind::Remove);
        assert_eq!(events[0].path, PathBuf::from("/nonexistent/file.txt"));
    }

    #[tokio::test]
    async fn test_scan_detects_changed_files() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // start_time 기록
        let start_time = SystemTime::now();

        // start_time 이후에 파일 생성
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file_path, "hello").unwrap();

        let mut path_manager = PathManager::default();
        path_manager.update(
            (vec![file_path.clone()].into_iter(), vec![].into_iter()),
            (vec![].into_iter(), vec![].into_iter()),
            (vec![].into_iter(), vec![].into_iter()),
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut scanner = Scanner::new(tx);

        scanner.scan(&path_manager, start_time);
        scanner.close();

        let mut events = Vec::new();
        while let Some(batch) = rx.recv().await {
            events.extend(batch);
        }

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, FsEventKind::Change);
        assert_eq!(events[0].path, file_path);
    }
}
