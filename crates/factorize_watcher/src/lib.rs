mod disk_watcher;
mod executor;
pub mod ignored;
pub mod paths;
mod scanner;
mod trigger;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use disk_watcher::DiskWatcher;
use executor::Executor;
pub use ignored::FsWatcherIgnored;
use paths::PathManager;
use scanner::Scanner;
use tokio::sync::mpsc;
use trigger::Trigger;

/// 파일 시스템 이벤트 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsEventKind {
    Change,
    Remove,
    Create,
}

/// 파일 시스템 이벤트
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEvent {
    pub path: PathBuf,
    pub kind: FsEventKind,
}

pub(crate) type EventBatch = Vec<FsEvent>;

/// 집계된 이벤트를 처리하는 trait (aggregate timeout 후 한번에 호출)
pub trait EventAggregateHandler: Send {
    fn on_aggregate(&self, changed: Vec<String>, deleted: Vec<String>);
}

/// 개별 이벤트를 즉시 처리하는 trait
pub trait EventHandler: Send {
    fn on_change(&self, path: String);
    fn on_delete(&self, path: String);
}

/// FsWatcher 설정
#[derive(Debug, Default)]
pub struct FsWatcherOptions {
    /// aggregate timeout (ms). 기본값 50ms
    pub aggregate_timeout: Option<u32>,
    /// 심볼릭 링크를 따라갈지 여부
    pub follow_symlinks: bool,
    /// 폴링 간격 (ms). VM/NFS 환경에서 사용
    pub poll_interval: Option<u32>,
}

/// FsWatcher - rspack의 FsWatcher에 대응
///
/// Step 1: DiskWatcher + Executor + Ignored
/// Step 2: + PathManager + Scanner + Trigger
pub struct FsWatcher {
    path_manager: PathManager,
    disk_watcher: DiskWatcher,
    executor: Executor,
    scanner: Scanner,
    trigger: Arc<Trigger>,
}

impl FsWatcher {
    pub fn new(options: FsWatcherOptions, ignored: FsWatcherIgnored) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let ignored_arc = Arc::new(ignored);
        let trigger = Arc::new(Trigger::new(tx.clone()));
        let disk_watcher = DiskWatcher::new(
            options.follow_symlinks,
            options.poll_interval,
            Arc::clone(&ignored_arc),
            tx.clone(),
        );
        let executor = Executor::new(rx, options.aggregate_timeout);
        let scanner = Scanner::new(tx);

        // ignored를 Arc에서 꺼내서 PathManager에 넘김
        let path_manager = PathManager::new(
            Arc::try_unwrap(ignored_arc).unwrap_or_default(),
        );

        Self {
            path_manager,
            disk_watcher,
            executor,
            scanner,
            trigger,
        }
    }

    /// 단순 경로 목록으로 감시 (기존 API 호환)
    pub async fn watch(
        &mut self,
        paths: Vec<PathBuf>,
        event_aggregate_handler: Box<dyn EventAggregateHandler>,
        event_handler: Box<dyn EventHandler>,
    ) -> Result<(), String> {
        self.disk_watcher.watch_paths(paths)?;
        self.executor
            .wait_for_execute(event_aggregate_handler, event_handler)
            .await;
        Ok(())
    }

    /// files/directories/missing 분리해서 감시 (rspack 스타일)
    pub async fn watch_with_paths(
        &mut self,
        files: (impl Iterator<Item = PathBuf>, impl Iterator<Item = PathBuf>),
        directories: (impl Iterator<Item = PathBuf>, impl Iterator<Item = PathBuf>),
        missing: (impl Iterator<Item = PathBuf>, impl Iterator<Item = PathBuf>),
        start_time: SystemTime,
        event_aggregate_handler: Box<dyn EventAggregateHandler>,
        event_handler: Box<dyn EventHandler>,
    ) -> Result<(), String> {
        self.path_manager.reset();
        self.path_manager.update(files, directories, missing);

        // Scanner: start_time 이전 변경분 감지
        self.scanner.scan(&self.path_manager, start_time);

        // DiskWatcher: 모든 경로를 감시 등록
        let all_paths: Vec<PathBuf> = self.path_manager.access().all().cloned().collect();
        self.disk_watcher.watch_paths(all_paths)?;

        // Executor: 이벤트 루프 시작
        self.executor
            .wait_for_execute(event_aggregate_handler, event_handler)
            .await;

        Ok(())
    }

    /// watcher 일시정지
    pub fn pause(&self) {
        self.executor.pause();
    }

    /// watcher 종료
    pub async fn close(&mut self) {
        self.disk_watcher.close();
        self.scanner.close();
        self.executor.close().await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tokio::time::{Duration, sleep};

    use super::*;

    struct TestAggregateHandler {
        changed: Arc<Mutex<Vec<String>>>,
        deleted: Arc<Mutex<Vec<String>>>,
    }

    impl EventAggregateHandler for TestAggregateHandler {
        fn on_aggregate(&self, changed: Vec<String>, deleted: Vec<String>) {
            self.changed.lock().unwrap().extend(changed);
            self.deleted.lock().unwrap().extend(deleted);
        }
    }

    struct TestEventHandler {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl EventHandler for TestEventHandler {
        fn on_change(&self, path: String) {
            self.events.lock().unwrap().push(format!("change:{path}"));
        }
        fn on_delete(&self, path: String) {
            self.events.lock().unwrap().push(format!("delete:{path}"));
        }
    }

    #[tokio::test]
    async fn test_watch_file_change() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let changed = Arc::new(Mutex::new(Vec::new()));
        let deleted = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));

        let mut watcher = FsWatcher::new(
            FsWatcherOptions {
                aggregate_timeout: Some(100),
                ..Default::default()
            },
            FsWatcherIgnored::None,
        );

        watcher
            .watch(
                vec![temp_dir.path().to_path_buf()],
                Box::new(TestAggregateHandler {
                    changed: Arc::clone(&changed),
                    deleted: Arc::clone(&deleted),
                }),
                Box::new(TestEventHandler {
                    events: Arc::clone(&events),
                }),
            )
            .await
            .unwrap();

        sleep(Duration::from_millis(200)).await;

        std::fs::write(&file_path, "world").unwrap();

        sleep(Duration::from_millis(500)).await;

        let changed_files = changed.lock().unwrap();
        let individual_events = events.lock().unwrap();

        assert!(
            !changed_files.is_empty() || !individual_events.is_empty(),
            "Expected file change events, got none. changed={changed_files:?}, events={individual_events:?}"
        );

        watcher.close().await;
    }

    #[tokio::test]
    async fn test_ignored_filters_events() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let nm_dir = temp_dir.path().join("node_modules");
        std::fs::create_dir_all(&nm_dir).unwrap();
        let nm_file = nm_dir.join("lodash.js");
        std::fs::write(&nm_file, "module.exports = {}").unwrap();

        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src_file = src_dir.join("index.ts");
        std::fs::write(&src_file, "console.log('hello')").unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));

        let mut watcher = FsWatcher::new(
            FsWatcherOptions {
                aggregate_timeout: Some(100),
                ..Default::default()
            },
            FsWatcherIgnored::Path("**/node_modules/**".to_string()),
        );

        watcher
            .watch(
                vec![temp_dir.path().to_path_buf()],
                Box::new(TestAggregateHandler {
                    changed: Arc::new(Mutex::new(Vec::new())),
                    deleted: Arc::new(Mutex::new(Vec::new())),
                }),
                Box::new(TestEventHandler {
                    events: Arc::clone(&events),
                }),
            )
            .await
            .unwrap();

        sleep(Duration::from_millis(200)).await;

        std::fs::write(&nm_file, "module.exports = { updated: true }").unwrap();
        std::fs::write(&src_file, "console.log('world')").unwrap();

        sleep(Duration::from_millis(500)).await;

        let all_events = events.lock().unwrap();

        let has_nm = all_events.iter().any(|e| e.contains("node_modules"));
        assert!(!has_nm, "node_modules events should be filtered: {all_events:?}");

        let has_src = all_events.iter().any(|e| e.contains("src"));
        assert!(has_src, "src events should be present: {all_events:?}");

        watcher.close().await;
    }

    #[tokio::test]
    async fn test_watch_with_paths_scanner() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let start_time = SystemTime::now();

        // start_time 이후에 파일 생성
        std::thread::sleep(std::time::Duration::from_millis(50));
        let file_path = temp_dir.path().join("changed.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let changed = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));

        let mut watcher = FsWatcher::new(
            FsWatcherOptions {
                aggregate_timeout: Some(100),
                ..Default::default()
            },
            FsWatcherIgnored::None,
        );

        watcher
            .watch_with_paths(
                (vec![file_path.clone()].into_iter(), vec![].into_iter()),
                (
                    vec![temp_dir.path().to_path_buf()].into_iter(),
                    vec![].into_iter(),
                ),
                (vec![].into_iter(), vec![].into_iter()),
                start_time,
                Box::new(TestAggregateHandler {
                    changed: Arc::clone(&changed),
                    deleted: Arc::new(Mutex::new(Vec::new())),
                }),
                Box::new(TestEventHandler {
                    events: Arc::clone(&events),
                }),
            )
            .await
            .unwrap();

        // Scanner가 start_time 이후 변경을 감지할 시간
        sleep(Duration::from_millis(300)).await;

        let changed_files = changed.lock().unwrap();
        let individual_events = events.lock().unwrap();

        // Scanner가 start_time 이후 생성된 파일을 감지해야 함
        let detected = changed_files
            .iter()
            .any(|f| f.contains("changed.txt"))
            || individual_events
                .iter()
                .any(|e| e.contains("changed.txt"));

        assert!(
            detected,
            "Scanner should detect file created after start_time. changed={changed_files:?}, events={individual_events:?}"
        );

        watcher.close().await;
    }
}
