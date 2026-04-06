mod disk_watcher;
mod executor;

use std::path::PathBuf;
use disk_watcher::DiskWatcher;
use executor::Executor;
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
}

/// 최소 단위 FsWatcher
/// rspack의 FsWatcher에서 핵심만 추출:
/// - DiskWatcher: notify crate 래핑
/// - Executor: 이벤트 집계 + 핸들러 호출
pub struct FsWatcher {
    disk_watcher: DiskWatcher,
    executor: Executor,
}

impl FsWatcher {
    pub fn new(options: FsWatcherOptions) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let disk_watcher = DiskWatcher::new(tx);
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
        self.disk_watcher.watch(paths)?;
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

        let mut watcher = FsWatcher::new(FsWatcherOptions {
            aggregate_timeout: Some(100),
        });

        // watch()는 executor를 spawn하고 바로 리턴
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

        // watcher가 OS에 등록될 시간
        sleep(Duration::from_millis(200)).await;

        // 파일 수정
        std::fs::write(&file_path, "world").unwrap();

        // aggregate timeout + 여유 시간
        sleep(Duration::from_millis(500)).await;

        let changed_files = changed.lock().unwrap();
        let individual_events = events.lock().unwrap();

        assert!(
            !changed_files.is_empty() || !individual_events.is_empty(),
            "Expected file change events, got none. changed={changed_files:?}, events={individual_events:?}"
        );

        watcher.close().await;
    }
}
