mod native_watcher;
mod watcher;

use std::path::PathBuf;
use std::sync::Arc;

use factorize_core::{Bundler, BundlerOptions, Plugin};
use napi::bindgen_prelude::*;
use napi::Either;
use napi_derive::napi;

/// JS가 `string|null` 또는 `Promise<string|null>` 반환 — 둘 다 받는 maybe-async tsfn
type TransformTsfn = napi::threadsafe_function::ThreadsafeFunction<
    FnArgs<(String, String)>,
    Either<Promise<Option<String>>, Option<String>>,
    FnArgs<(String, String)>,
    Status,
    false,
>;

#[napi(object)]
pub struct BindingBundlerOptions {
    pub input: String,
}

#[napi(object)]
pub struct BindingOutput {
    pub code: String,
    pub modules: Vec<String>,
}

/// JS에 노출되는 thin 진입점. 파이프라인은 전부 factorize_core가 소유한다
#[napi]
pub struct BindingBundler {
    input: String,
    plugins: Vec<Arc<dyn Plugin>>,
}

#[napi]
impl BindingBundler {
    #[napi(constructor)]
    pub fn new(options: BindingBundlerOptions) -> Self {
        Self { input: options.input, plugins: vec![] }
    }

    /// JS plugin의 moduleParsed 훅 등록 (Rust 파이프라인이 스캔 중 호출)
    #[napi]
    pub fn on_module_parsed(
        &mut self,
        #[napi(ts_arg_type = "(id: string) => void")] callback: Function<'static>,
    ) -> napi::Result<()> {
        self.plugins.push(Arc::new(JsPlugin::new(callback)?));
        Ok(())
    }

    /// JS plugin의 transform 훅 등록 (Rust가 코드를 보내 변형 결과를 await)
    #[napi(
        ts_args_type = "callback: (code: string, id: string) => string | null | Promise<string | null>"
    )]
    pub fn on_transform(&mut self, callback: TransformTsfn) -> napi::Result<()> {
        self.plugins.push(Arc::new(TransformJsPlugin { transform: callback }));
        Ok(())
    }

    /// async → JS Promise. core Bundler::build()로 위임
    #[napi]
    pub async fn build(&self) -> napi::Result<BindingOutput> {
        let options = BundlerOptions { input: PathBuf::from(self.input.clone()) };
        let output = Bundler::with_plugins(options, self.plugins.clone())
            .build()
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(BindingOutput { code: output.code, modules: output.modules })
    }
}

struct JsPlugin {
    on_module_parsed: napi::threadsafe_function::ThreadsafeFunction<
        String,
        Unknown<'static>,
        String,
        Status,
        false,
        false,
        0,
    >,
}

impl JsPlugin {
    fn new(callback: Function<'static>) -> napi::Result<Self> {
        let on_module_parsed = callback
            .build_threadsafe_function::<String>()
            .weak::<false>()
            .max_queue_size::<0>()
            .build_callback(
                move |ctx: napi::threadsafe_function::ThreadsafeCallContext<_>| Ok(ctx.value),
            )?;
        Ok(Self { on_module_parsed })
    }
}

#[async_trait::async_trait]
impl Plugin for JsPlugin {
    async fn module_parsed(&self, id: &str) -> anyhow::Result<()> {
        self.on_module_parsed.call(
            id.to_string(),
            napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
        );
        Ok(())
    }
}

struct TransformJsPlugin {
    transform: TransformTsfn,
}

#[async_trait::async_trait]
impl Plugin for TransformJsPlugin {
    async fn transform(&self, code: &str, id: &str) -> anyhow::Result<Option<String>> {
        let args = FnArgs { data: (code.to_string(), id.to_string()) };
        let ret = self
            .transform
            .call_async(args)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        match ret {
            Either::A(promise) => promise.await.map_err(|e| anyhow::anyhow!(e.to_string())),
            Either::B(value) => Ok(value),
        }
    }
}
