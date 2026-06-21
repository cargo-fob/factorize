use anyhow::Result;

/// 번들러 훅. Rust core가 호출 — 구현이 Rust든 (napi 통해) JS든 무관
#[async_trait::async_trait]
pub trait Plugin: Send + Sync {
    /// 모듈 스캔마다 호출되는 notification
    async fn module_parsed(&self, _id: &str) -> Result<()> {
        Ok(())
    }

    /// 모듈 소스 변형 (Some이면 교체)
    async fn transform(&self, _code: &str, _id: &str) -> Result<Option<String>> {
        Ok(None)
    }
}
