use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind};
use tokio::sync::mpsc::UnboundedSender;

use crate::ignored::FsWatcherIgnored;
use crate::{EventBatch, FsEvent, FsEventKind};

/// 감시 패턴: 경로 + 재귀 모드
/// rspack의 WatchPattern과 동일
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct WatchPattern {
    pub path: PathBuf,
    pub mode: RecursiveMode,
}

/// DiskWatcher는 notify crate의 RecommendedWatcher를 래핑한다.
/// rspack 대비 추가된 기능:
/// - follow_symlinks / poll_interval 옵션
/// - WatchPattern 추적 + stale pattern unwatch
/// - ignored 패턴 필터링
pub struct DiskWatcher {
    inner: Option<RecommendedWatcher>,
    watch_patterns: HashSet<WatchPattern>,
}

impl DiskWatcher {
    pub fn new(
        follow_symlinks: bool,
        poll_interval: Option<u32>,
        ignored: Arc<FsWatcherIgnored>,
        tx: UnboundedSender<EventBatch>,
    ) -> Self {
        let config = match poll_interval {
            Some(poll) => notify::Config::default()
                .with_follow_symlinks(follow_symlinks)
                .with_poll_interval(Duration::from_millis(u64::from(poll))),
            None => notify::Config::default().with_follow_symlinks(follow_symlinks),
        };

        let inner = RecommendedWatcher::new(
            move |result: notify::Result<Event>| match result {
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

                    // ignored 패턴 필터링
                    let batch: EventBatch = event
                        .paths
                        .into_iter()
                        .filter(|path| {
                            !ignored.should_ignore(&path.to_string_lossy())
                        })
                        .map(|path| FsEvent { path, kind })
                        .collect();

                    if !batch.is_empty() {
                        let _ = tx.send(batch);
                    }
                }
                Err(e) => {
                    eprintln!("Error in file watcher: {e:?}");
                }
            },
            config,
        )
        .expect("Failed to create disk watcher");

        Self {
            inner: Some(inner),
            watch_patterns: HashSet::new(),
        }
    }

    /// 새 패턴으로 감시. 기존에 있던 stale 패턴은 자동 unwatch.
    pub fn watch(
        &mut self,
        patterns: impl Iterator<Item = WatchPattern>,
    ) -> Result<(), String> {
        let new_patterns: HashSet<WatchPattern> = patterns.collect();
        let new_paths: HashSet<&PathBuf> = new_patterns.iter().map(|p| &p.path).collect();

        // stale 패턴 제거 (더 이상 필요 없는 경로)
        let stale_paths: Vec<PathBuf> = self
            .watch_patterns
            .iter()
            .filter(|p| !new_paths.contains(&p.path))
            .map(|p| p.path.clone())
            .collect();

        for path in &stale_paths {
            if let Some(watcher) = &mut self.inner {
                if let Err(e) = watcher.unwatch(path) {
                    if !matches!(e.kind, notify::ErrorKind::WatchNotFound) {
                        return Err(format!("Failed to unwatch {}: {e}", path.display()));
                    }
                }
            }
        }

        self.watch_patterns
            .retain(|p| !stale_paths.contains(&p.path));

        // 새 패턴 등록
        for pattern in new_patterns {
            if self.watch_patterns.contains(&pattern) {
                continue;
            }

            if let Some(watcher) = &mut self.inner {
                watcher
                    .watch(&pattern.path, pattern.mode)
                    .map_err(|e| format!("Failed to watch {}: {e}", pattern.path.display()))?;
            }

            self.watch_patterns.insert(pattern);
        }

        Ok(())
    }

    /// 단순 경로 목록으로 감시 (기존 API 호환)
    pub fn watch_paths(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        let patterns = paths.into_iter().map(|path| WatchPattern {
            path,
            mode: RecursiveMode::Recursive,
        });
        self.watch(patterns)
    }

    pub fn close(&mut self) {
        drop(self.inner.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_watch_removes_stale_patterns() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let ignored = Arc::new(FsWatcherIgnored::None);
        let mut watcher = DiskWatcher::new(false, None, ignored, tx);

        let temp_dir = tempfile::TempDir::new().unwrap();
        let base = temp_dir.path().canonicalize().unwrap();

        let dir_a = base.join("a");
        let dir_b = base.join("b");
        let dir_c = base.join("c");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::create_dir_all(&dir_c).unwrap();

        // 첫 watch: {A, B}
        watcher
            .watch(
                vec![
                    WatchPattern {
                        path: dir_a.clone(),
                        mode: RecursiveMode::NonRecursive,
                    },
                    WatchPattern {
                        path: dir_b.clone(),
                        mode: RecursiveMode::NonRecursive,
                    },
                ]
                .into_iter(),
            )
            .unwrap();
        assert_eq!(watcher.watch_patterns.len(), 2);

        // 두번째 watch: {B, C} → A는 자동 unwatch
        watcher
            .watch(
                vec![
                    WatchPattern {
                        path: dir_b.clone(),
                        mode: RecursiveMode::NonRecursive,
                    },
                    WatchPattern {
                        path: dir_c.clone(),
                        mode: RecursiveMode::NonRecursive,
                    },
                ]
                .into_iter(),
            )
            .unwrap();

        assert_eq!(watcher.watch_patterns.len(), 2);
        let paths: HashSet<_> = watcher
            .watch_patterns
            .iter()
            .map(|p| p.path.clone())
            .collect();
        assert!(paths.contains(&dir_b));
        assert!(paths.contains(&dir_c));
        assert!(!paths.contains(&dir_a));
    }
}
