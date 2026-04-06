use std::path::PathBuf;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind};
use tokio::sync::mpsc::UnboundedSender;

use crate::{EventBatch, FsEvent, FsEventKind};

/// `DiskWatcher`는 notify crate의 RecommendedWatcher를 래핑한다.
/// rspack의 DiskWatcher에서 핵심만 추출:
/// - notify 이벤트를 FsEventKind로 변환
/// - 변환된 이벤트를 channel로 전송
pub struct DiskWatcher {
    inner: Option<RecommendedWatcher>,
}

impl DiskWatcher {
    pub fn new(tx: UnboundedSender<EventBatch>) -> Self {
        let inner = RecommendedWatcher::new(
            move |result: notify::Result<Event>| {
                match result {
                    Ok(event) => {
                        if event.paths.is_empty() {
                            return;
                        }

                        let kind = match event.kind {
                            EventKind::Create(_) => FsEventKind::Create,
                            EventKind::Modify(
                                ModifyKind::Data(_)
                                | ModifyKind::Any
                                | ModifyKind::Name(_)
                                | ModifyKind::Metadata(_),
                            ) => FsEventKind::Change,
                            EventKind::Remove(_) => FsEventKind::Remove,
                            _ => return,
                        };

                        let batch: EventBatch = event
                            .paths
                            .into_iter()
                            .map(|path| FsEvent { path, kind })
                            .collect();

                        let _ = tx.send(batch);
                    }
                    Err(e) => {
                        eprintln!("Error in file watcher: {e:?}");
                    }
                }
            },
            notify::Config::default(),
        )
        .expect("Failed to create disk watcher");

        Self { inner: Some(inner) }
    }

    pub fn watch(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        let watcher = self
            .inner
            .as_mut()
            .ok_or("Watcher already closed")?;

        for path in paths {
            watcher
                .watch(&path, RecursiveMode::Recursive)
                .map_err(|e| format!("Failed to watch {}: {e}", path.display()))?;
        }

        Ok(())
    }

    pub fn close(&mut self) {
        drop(self.inner.take());
    }
}
