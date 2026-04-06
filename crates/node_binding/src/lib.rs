mod native_watcher;

use napi_derive::napi;

#[napi]
fn sum(a: i32, b: i32) -> i32 {
  factorize_core::add(a as u64, b as u64) as i32
}

#[napi]
struct JsCompiler {
  name: String,
  entry: String,
}

#[napi]
impl JsCompiler {
  #[napi(constructor)]
  pub fn new(name: String, entry: String) -> Self {
    JsCompiler { name, entry }
  }

  #[napi]
  pub fn compile(&self) -> String {
    format!("[{}] compiling entry: {}", self.name, self.entry)
  }

  #[napi(getter)]
  pub fn name(&self) -> &str {
    &self.name
  }

  #[napi(getter)]
  pub fn entry(&self) -> &str {
    &self.entry
  }
}
