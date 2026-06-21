//! 번들러 코어 — Bundler가 scan → link → generate를 소유

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

mod plugin;
pub use plugin::Plugin;

#[derive(Debug, Clone)]
pub struct BundlerOptions {
    pub input: PathBuf,
}

#[derive(Debug)]
pub struct Module {
    pub id: String,
    pub code: String,
    pub deps: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ModuleGraph {
    pub modules: Vec<Module>,
}

#[derive(Debug)]
pub struct Output {
    pub code: String,
    pub modules: Vec<String>,
}

pub struct Bundler {
    options: BundlerOptions,
    plugins: Vec<Arc<dyn Plugin>>,
}

impl Bundler {
    pub fn new(options: BundlerOptions) -> Self {
        Self { options, plugins: vec![] }
    }

    pub fn with_plugins(options: BundlerOptions, plugins: Vec<Arc<dyn Plugin>>) -> Self {
        Self { options, plugins }
    }

    pub async fn build(&self) -> Result<Output> {
        let graph = self.scan().await?;
        let graph = self.link(graph);
        Ok(self.generate(graph))
    }

    /// entry에서 상대 import를 따라가며 module graph를 만든다 (worklist)
    async fn scan(&self) -> Result<ModuleGraph> {
        let entry = self
            .options
            .input
            .canonicalize()
            .with_context(|| format!("entry를 찾을 수 없음: {}", self.options.input.display()))?
            .to_string_lossy()
            .into_owned();

        let mut graph = ModuleGraph::default();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([entry]);

        while let Some(id) = queue.pop_front() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let mut code =
                std::fs::read_to_string(&id).with_context(|| format!("읽기 실패: {id}"))?;

            for plugin in &self.plugins {
                if let Some(transformed) = plugin.transform(&code, &id).await? {
                    code = transformed;
                }
            }

            let mut deps = vec![];
            for spec in parse_imports(&code) {
                if let Some(dep_id) = resolve(&spec, &id) {
                    deps.push(dep_id.clone());
                    queue.push_back(dep_id);
                }
            }
            for plugin in &self.plugins {
                plugin.module_parsed(&id).await?;
            }
            graph.modules.push(Module { id, code, deps });
        }
        Ok(graph)
    }

    /// 실행 순서 결정 (deps-first가 되도록 역순)
    fn link(&self, mut graph: ModuleGraph) -> ModuleGraph {
        graph.modules.reverse();
        graph
    }

    /// 모듈 코드를 이어붙인다
    fn generate(&self, graph: ModuleGraph) -> Output {
        let mut code = String::new();
        for m in &graph.modules {
            code.push_str(&format!("// === {} ===\n{}\n\n", m.id, m.code.trim_end()));
        }
        Output { code, modules: graph.modules.iter().map(|m| m.id.clone()).collect() }
    }
}

/// 상대 import만 추출 (naive — 진짜 파서 아님)
fn parse_imports(code: &str) -> Vec<String> {
    let mut specs = vec![];
    for line in code.lines() {
        let line = line.trim();
        for marker in ["from ", "import ", "require("] {
            if let Some(rest) = line.find(marker).map(|i| &line[i + marker.len()..]) {
                if let Some(spec) = extract_quoted(rest) {
                    if spec.starts_with('.') {
                        specs.push(spec);
                    }
                }
            }
        }
    }
    specs
}

fn extract_quoted(s: &str) -> Option<String> {
    let s = s.trim_start_matches(['(', ' ']);
    let quote = s.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &s[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// 상대 import를 절대 경로로 (후보 확장자 시도)
fn resolve(spec: &str, importer: &str) -> Option<String> {
    let dir = Path::new(importer).parent()?;
    let base = dir.join(spec);
    let candidates = [
        base.clone(),
        base.with_extension("js"),
        base.with_extension("ts"),
        base.join("index.js"),
        base.join("index.ts"),
    ];
    candidates
        .iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().into_owned())
}
