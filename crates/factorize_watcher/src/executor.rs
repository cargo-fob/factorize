use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Mutex;

use crate::{EventAggregateHandler, EventBatch, EventHandler, FsEventKind};

const DEFAULT_AGGREGATE_TIMEOUT: u32 = 50;

/// 집계된 파일 데이터
#[derive(Debug, Default)]
struct FilesData {
    changed: HashSet<String>,
    deleted: HashSet<String>,
}

impl FilesData {
    fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.deleted.is_empty()
    }
}

/// `Executor`는 이벤트를 수신하고 aggregate timeout 후 핸들러를 호출한다.
/// rspack의 Executor에서 핵심만 추출:
/// - 이벤트 수신 → changed/deleted 분류
/// - aggregate timeout 후 집계된 결과를 핸들러에 전달
/// - 개별 이벤트도 즉시 핸들러에 전달
pub struct Executor {
    aggregate_timeout: u32,
    rx: Arc<Mutex<UnboundedReceiver<EventBatch>>>,
    files_data: Arc<Mutex<FilesData>>,
    paused: Arc<AtomicBool>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Executor {
    pub fn new(rx: UnboundedReceiver<EventBatch>, aggregate_timeout: Option<u32>) -> Self {
        Self {
            aggregate_timeout: aggregate_timeout.unwrap_or(DEFAULT_AGGREGATE_TIMEOUT),
            rx: Arc::new(Mutex::new(rx)),
            files_data: Arc::new(Mutex::new(FilesData::default())),
            paused: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    pub async fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    /// 이벤트 루프 시작: 이벤트를 수신하고 핸들러를 호출
    pub async fn wait_for_execute(
        &mut self,
        event_aggregate_handler: Box<dyn EventAggregateHandler>,
        event_handler: Box<dyn EventHandler>,
    ) {
        let rx = Arc::clone(&self.rx);
        let files_data = Arc::clone(&self.files_data);
        let paused = Arc::clone(&self.paused);
        let aggregate_timeout = self.aggregate_timeout as u64;

        let handle = tokio::spawn(async move {
            // aggregate timer 상태
            let mut aggregate_deadline: Option<tokio::time::Instant> = None;

            loop {
                let timeout = match aggregate_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline),
                    None => tokio::time::sleep(tokio::time::Duration::from_secs(86400)),
                };

                tokio::select! {
                    // 새 이벤트 수신
                    event = async { rx.lock().await.recv().await } => {
                        match event {
                            Some(batch) => {
                                // 개별 이벤트 즉시 전달
                                for ev in &batch {
                                    let path = ev.path.to_string_lossy().to_string();
                                    match ev.kind {
                                        FsEventKind::Change | FsEventKind::Create => {
                                            event_handler.on_change(path.clone());
                                            files_data.lock().await.changed.insert(path);
                                        }
                                        FsEventKind::Remove => {
                                            event_handler.on_delete(path.clone());
                                            files_data.lock().await.deleted.insert(path);
                                        }
                                    }
                                }

                                // aggregate timer 시작/리셋
                                if !paused.load(Ordering::Relaxed) {
                                    aggregate_deadline = Some(
                                        tokio::time::Instant::now()
                                            + tokio::time::Duration::from_millis(aggregate_timeout),
                                    );
                                }
                            }
                            None => break, // channel closed
                        }
                    }
                    // aggregate timeout 도달 → 집계 결과 전달
                    _ = timeout, if aggregate_deadline.is_some() => {
                        aggregate_deadline = None;

                        let data = {
                            let mut files = files_data.lock().await;
                            if files.is_empty() {
                                continue;
                            }
                            std::mem::take(&mut *files)
                        };

                        event_aggregate_handler.on_aggregate(
                            data.changed.into_iter().collect(),
                            data.deleted.into_iter().collect(),
                        );
                    }
                }
            }
        });

        self.handle = Some(handle);
    }
}
