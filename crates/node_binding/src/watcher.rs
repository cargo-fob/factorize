//! Rust가 watch 루프를 소유 — FsWatcher(감시) + Bundler(rebuild)를 들고,
//! 변경 시 Rust에서 rebuild → 이벤트를 JS로 push

use std::path::{Path, PathBuf};
use std::sync::Arc;

use factorize_core::{Bundler, BundlerOptions};
use factorize_watcher::{
    EventAggregateHandler, EventHandler, FsWatcher, FsWatcherIgnored, FsWatcherOptions,
};
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;

use crate::BindingBundlerOptions;

type EventTsfn =
    ThreadsafeFunction<BindingWatchEvent, Unknown<'static>, BindingWatchEvent, Status, false, false, 0>;

#[napi(object)]
pub struct BindingWatchEvent {
    /// "change" | "bundle_end" | "error"
    pub kind: String,
    pub path: Option<String>,
    pub modules: Option<Vec<String>>,
    pub code: Option<String>,
    pub error: Option<String>,
}

impl BindingWatchEvent {
    fn change(path: String) -> Self {
        Self { kind: "change".into(), path: Some(path), modules: None, code: None, error: None }
    }
    fn bundle_end(modules: Vec<String>, code: String) -> Self {
        Self { kind: "bundle_end".into(), path: None, modules: Some(modules), code: Some(code), error: None }
    }
    fn error(msg: String) -> Self {
        Self { kind: "error".into(), path: None, modules: None, code: None, error: Some(msg) }
    }
}

fn build_event_tsfn(callback: Function<'static>) -> napi::Result<EventTsfn> {
    callback
        .build_threadsafe_function::<BindingWatchEvent>()
        .weak::<false>()
        .max_queue_size::<0>()
        .build_callback(move |ctx: napi::threadsafe_function::ThreadsafeCallContext<_>| Ok(ctx.value))
}

async fn rebuild_and_emit(input: String, tsfn: Arc<EventTsfn>) {
    let options = BundlerOptions { input: PathBuf::from(input) };
    let event = match Bundler::new(options).build().await {
        Ok(out) => BindingWatchEvent::bundle_end(out.modules, out.code),
        Err(e) => BindingWatchEvent::error(e.to_string()),
    };
    tsfn.call(event, ThreadsafeFunctionCallMode::NonBlocking);
}

/// 변경 burst마다 rebuild (aggregate timeout 후 1회)
struct RebuildHandler {
    input: String,
    tsfn: Arc<EventTsfn>,
}
impl EventAggregateHandler for RebuildHandler {
    fn on_aggregate(&self, _changed: Vec<String>, _deleted: Vec<String>) {
        let input = self.input.clone();
        let tsfn = Arc::clone(&self.tsfn);
        spawn(async move { rebuild_and_emit(input, tsfn).await });
    }
}

/// 개별 변경을 즉시 "change" 이벤트로
struct ChangeHandler {
    tsfn: Arc<EventTsfn>,
}
impl EventHandler for ChangeHandler {
    fn on_change(&self, path: String) {
        self.tsfn.call(BindingWatchEvent::change(path), ThreadsafeFunctionCallMode::NonBlocking);
    }
    fn on_delete(&self, path: String) {
        self.tsfn.call(BindingWatchEvent::change(path), ThreadsafeFunctionCallMode::NonBlocking);
    }
}

#[napi]
pub struct BindingWatcher {
    input: String,
    watcher: FsWatcher,
    tsfn: Arc<EventTsfn>,
}

#[napi]
impl BindingWatcher {
    #[napi(
        constructor,
        ts_args_type = "options: BindingBundlerOptions, listener: (event: BindingWatchEvent) => void"
    )]
    pub fn new(
        options: BindingBundlerOptions,
        listener: Function<'static>,
    ) -> napi::Result<Self> {
        let tsfn = Arc::new(build_event_tsfn(listener)?);
        let watcher = FsWatcher::new(
            FsWatcherOptions {
                aggregate_timeout: Some(100),
                follow_symlinks: false,
                poll_interval: None,
            },
            FsWatcherIgnored::None,
        );
        Ok(Self { input: options.input, watcher, tsfn })
    }

    /// 초기 빌드 1회 + 이후 변경마다 Rust에서 rebuild.
    /// tsfn weak=false라 Node 이벤트루프가 살아있어 watch가 유지된다
    #[napi]
    pub fn run(&mut self, reference: Reference<BindingWatcher>, env: Env) -> napi::Result<()> {
        let dir = PathBuf::from(&self.input)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let init_input = self.input.clone();
        let init_tsfn = Arc::clone(&self.tsfn);
        spawn(async move { rebuild_and_emit(init_input, init_tsfn).await });

        let aggregate = RebuildHandler { input: self.input.clone(), tsfn: Arc::clone(&self.tsfn) };
        let change = ChangeHandler { tsfn: Arc::clone(&self.tsfn) };

        reference.share_with(env, move |bw| {
            spawn(async move {
                let _ = bw
                    .watcher
                    .watch(vec![dir], Box::new(aggregate), Box::new(change))
                    .await;
            });
            Ok(())
        })?;
        Ok(())
    }

    #[napi]
    pub async unsafe fn close(&mut self) -> napi::Result<()> {
        self.watcher.close().await;
        Ok(())
    }
}
