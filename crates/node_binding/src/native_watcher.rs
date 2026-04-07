use std::path::PathBuf;

use factorize_watcher::{
    EventAggregateHandler, EventHandler, FsWatcher, FsWatcherIgnored, FsWatcherOptions,
};
use napi::bindgen_prelude::*;
use napi_derive::napi;

/// JS에서 받는 watcher 옵션
#[napi(object, object_to_js = false)]
pub struct JsWatcherOptions {
  pub aggregate_timeout: Option<u32>,
  pub follow_symlinks: Option<bool>,
  pub poll_interval: Option<u32>,
  /// glob 패턴 목록 (e.g. ["node_modules", ".git"])
  pub ignored: Option<Vec<String>>,
}

/// JS에 전달하는 집계 결과
#[napi]
pub struct JsWatchResult {
  pub changed_files: Vec<String>,
  pub removed_files: Vec<String>,
}

/// rspack의 NativeWatcher 최소 버전
/// JS에서: new NativeWatcher({ aggregateTimeout: 50 })
#[napi]
pub struct NativeWatcher {
  watcher: FsWatcher,
  closed: bool,
}

#[napi]
impl NativeWatcher {
  #[napi(constructor)]
  pub fn new(options: JsWatcherOptions) -> Self {
    let ignored = match options.ignored {
      Some(paths) if !paths.is_empty() => FsWatcherIgnored::Paths(paths),
      _ => FsWatcherIgnored::None,
    };

    let watcher = FsWatcher::new(
      FsWatcherOptions {
        aggregate_timeout: options.aggregate_timeout,
        follow_symlinks: options.follow_symlinks.unwrap_or(false),
        poll_interval: options.poll_interval,
      },
      ignored,
    );

    Self {
      watcher,
      closed: false,
    }
  }

  /// JS에서: watcher.watch(["/path/to/dir"], callback, callbackUndelayed)
  #[napi]
  pub fn watch(
    &mut self,
    reference: Reference<NativeWatcher>,
    paths: Vec<String>,
    #[napi(ts_arg_type = "(err: Error | null, result: JsWatchResult) => void")]
    callback: Function<'static>,
    #[napi(ts_arg_type = "(path: string) => void")] callback_undelayed: Function<'static>,
    env: Env,
  ) -> napi::Result<()> {
    if self.closed {
      return Err(napi::Error::from_reason("Watcher already closed"));
    }

    let aggregate_handler = JsAggregateHandler::new(callback)?;
    let event_handler = JsEventHandlerUndelayed::new(callback_undelayed)?;

    let watch_paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();

    reference.share_with(env, |native_watcher| {
      napi::bindgen_prelude::spawn(async move {
        let _ = native_watcher
          .watcher
          .watch(
            watch_paths,
            Box::new(aggregate_handler),
            Box::new(event_handler),
          )
          .await;
      });
      Ok(())
    })?;

    Ok(())
  }

  #[napi]
  pub fn pause(&self) -> napi::Result<()> {
    self.watcher.pause();
    Ok(())
  }

  #[napi]
  pub async unsafe fn close(&mut self) -> napi::Result<()> {
    self.watcher.close().await;
    self.closed = true;
    Ok(())
  }
}

// --- JS 콜백 래퍼들 (rspack napi v3 스타일) ---

/// 집계된 이벤트를 JS callback으로 전달 (aggregate timeout 후)
/// rspack: JsEventHandler
struct JsAggregateHandler {
  inner: napi::threadsafe_function::ThreadsafeFunction<
    JsWatchResult,
    Unknown<'static>,
    JsWatchResult,
    Status,
    true,
    true,
    1,
  >,
}

impl JsAggregateHandler {
  fn new(callback: Function<'static>) -> napi::Result<Self> {
    let inner = callback
      .build_threadsafe_function::<JsWatchResult>()
      .callee_handled::<true>()
      .max_queue_size::<1>()
      .weak::<true>()
      .build_callback(
        move |ctx: napi::threadsafe_function::ThreadsafeCallContext<_>| Ok(ctx.value),
      )?;

    Ok(Self { inner })
  }
}

impl EventAggregateHandler for JsAggregateHandler {
  fn on_aggregate(&self, changed: Vec<String>, deleted: Vec<String>) {
    let result = JsWatchResult {
      changed_files: changed,
      removed_files: deleted,
    };
    self.inner.call(
      Ok(result),
      napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
    );
  }
}

/// 개별 이벤트를 JS callback으로 즉시 전달
/// rspack: JsEventHandlerUndelayed
struct JsEventHandlerUndelayed {
  inner: napi::threadsafe_function::ThreadsafeFunction<
    String,
    Unknown<'static>,
    String,
    Status,
    false,
    false,
    1,
  >,
}

impl JsEventHandlerUndelayed {
  fn new(callback: Function<'static>) -> napi::Result<Self> {
    let inner = callback
      .build_threadsafe_function::<String>()
      .weak::<false>()
      .max_queue_size::<1>()
      .build_callback(
        move |ctx: napi::threadsafe_function::ThreadsafeCallContext<_>| Ok(ctx.value),
      )?;

    Ok(Self { inner })
  }
}

impl EventHandler for JsEventHandlerUndelayed {
  fn on_change(&self, path: String) {
    self.inner.call(
      path,
      napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
    );
  }

  fn on_delete(&self, path: String) {
    self.inner.call(
      path,
      napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
    );
  }
}
