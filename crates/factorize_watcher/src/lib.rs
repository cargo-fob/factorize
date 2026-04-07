mod disk_watcher;
mod executor;
pub mod ignored;

use std::path::PathBuf;
use std::sync::Arc;

use disk_watcher::DiskWatcher;
use executor::Executor;
pub use ignored::FsWatcherIgnored;
use tokio::sync::mpsc;

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

/// 최소 단위 FsWatcher
/// rspack의 FsWatcher에서 핵심 추출 + Step 1 고도화:
/// - DiskWatcher: notify crate 래핑 + ignored 필터링 + WatchPattern 관리
/// - Executor: 이벤트 집계 + 핸들러 호출
pub struct FsWatcher {
    disk_watcher: DiskWatcher,
    executor: Executor,
}

impl FsWatcher {
    pub fn new(options: FsWatcherOptions, ignored: FsWatcherIgnored) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let ignored = Arc::new(ignored);
        let disk_watcher = DiskWatcher::new(
            options.follow_symlinks,
            options.poll_interval,
            ignored,
            tx,
        );
        let executor = Executor::new(rx, options.aggregate_timeout);

        Self {
            disk_watcher,
            executor,
        }
    }

    /// 경로들을 감시하고 이벤트 핸들러를 등록한 뒤, 이벤트 루프 시작
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

    /// watcher 일시정지
    pub fn pause(&self) {
        self.executor.pause();
    }

    /// watcher 종료
    pub async fn close(&mut self) {
        self.disk_watcher.close();
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

        // node_modules 안에 파일 생성
        let nm_dir = temp_dir.path().join("node_modules");
        std::fs::create_dir_all(&nm_dir).unwrap();
        let nm_file = nm_dir.join("lodash.js");
        std::fs::write(&nm_file, "module.exports = {}").unwrap();

        // src 안에 파일 생성
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
            FsWatcherIgnored::Path("node_modules".to_string()),
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

        // 두 파일 모두 수정
        std::fs::write(&nm_file, "module.exports = { updated: true }").unwrap();
        std::fs::write(&src_file, "console.log('world')").unwrap();

        sleep(Duration::from_millis(500)).await;

        let all_events = events.lock().unwrap();

        // node_modules 이벤트는 없어야 함
        let has_nm = all_events.iter().any(|e| e.contains("node_modules"));
        assert!(!has_nm, "node_modules events should be filtered: {all_events:?}");

        // src 이벤트는 있어야 함
        let has_src = all_events.iter().any(|e| e.contains("src"));
        assert!(has_src, "src events should be present: {all_events:?}");

        watcher.close().await;
    }
}
