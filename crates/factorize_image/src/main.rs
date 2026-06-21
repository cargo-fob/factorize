//! 개빠른 Rust 이미지 최적화 CLI — optimize(1장) / watch(폴더)

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use factorize_image::{optimize, OptimizeOptions, OutFormat};
use factorize_watcher::{
    EventAggregateHandler, EventHandler, FsWatcher, FsWatcherIgnored, FsWatcherOptions,
};

#[derive(Parser)]
#[command(name = "factorize-img", about = "개빠른 Rust 이미지 최적화")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 이미지 1장 최적화
    Optimize {
        /// 입력 이미지 경로
        input: PathBuf,
        /// 출력 경로 (생략 시 <이름>.min.<포맷>)
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        opt: OptArgs,
    },
    /// 폴더를 감시하다 이미지가 바뀌면 자동 최적화
    Watch {
        /// 감시할 폴더
        dir: PathBuf,
        #[command(flatten)]
        opt: OptArgs,
    },
}

#[derive(Args, Clone)]
struct OptArgs {
    /// 최대 폭(px), 초과 시 비율 유지 축소
    #[arg(long)]
    width: Option<u32>,
    /// JPEG 품질 1~100
    #[arg(short, long, default_value_t = 75, value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,
    /// 출력 포맷
    #[arg(short, long, value_enum, default_value_t = CliFormat::Jpeg)]
    format: CliFormat,
}

impl OptArgs {
    fn to_options(&self) -> OptimizeOptions {
        OptimizeOptions { max_width: self.width, quality: self.quality, format: self.format.into() }
    }
}

/// core의 OutFormat에 clap 의존을 안 묻히려고 두는 CLI 전용 mirror
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliFormat {
    #[value(alias = "jpg")]
    Jpeg,
    Png,
    Webp,
}

impl From<CliFormat> for OutFormat {
    fn from(f: CliFormat) -> Self {
        match f {
            CliFormat::Jpeg => OutFormat::Jpeg,
            CliFormat::Png => OutFormat::Png,
            CliFormat::Webp => OutFormat::WebP,
        }
    }
}

struct Report {
    out: PathBuf,
    in_size: usize,
    out_size: usize,
    width: u32,
    height: u32,
    saved: f64,
    ms: f64,
}

fn process_file(input: &Path, output: Option<&Path>, opts: &OptimizeOptions) -> Result<Report> {
    let bytes =
        std::fs::read(input).with_context(|| format!("입력 못 읽음: {}", input.display()))?;
    let in_size = bytes.len();

    // 측정은 optimize만, IO 제외
    let started = Instant::now();
    let r = optimize(&bytes, opts)?;
    let ms = started.elapsed().as_secs_f64() * 1000.0;

    let out = output.map(Path::to_path_buf).unwrap_or_else(|| min_path(input, opts.format));
    std::fs::write(&out, &r.bytes).with_context(|| format!("출력 못 씀: {}", out.display()))?;

    let out_size = r.bytes.len();
    let saved = if in_size > 0 { 100.0 * (1.0 - out_size as f64 / in_size as f64) } else { 0.0 };
    Ok(Report { out, in_size, out_size, width: r.width, height: r.height, saved, ms })
}

fn min_path(input: &Path, format: OutFormat) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    input.with_file_name(format!("{stem}.min.{}", format.ext()))
}

fn is_image(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tiff")
    )
}

fn human(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.2} MB", b / (KB * KB))
    }
}

struct AutoOptimizer {
    opts: OptimizeOptions,
}

impl EventHandler for AutoOptimizer {
    fn on_change(&self, path: String) {
        let p = Path::new(&path);
        // .min. 출력물 재최적화 루프 방지 (1차 방어는 watcher ignored)
        if !is_image(p) || path.contains(".min.") {
            return;
        }
        match process_file(p, None, &self.opts) {
            Ok(r) => println!(
                "♻  {}  →  {}  {} → {} ({:.1}%↓, {:.0}ms)",
                p.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                r.out.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                human(r.in_size),
                human(r.out_size),
                r.saved,
                r.ms,
            ),
            Err(e) => eprintln!("✗  {path}: {e}"),
        }
    }

    fn on_delete(&self, _path: String) {}
}

struct NoopAggregate;
impl EventAggregateHandler for NoopAggregate {
    fn on_aggregate(&self, _changed: Vec<String>, _deleted: Vec<String>) {}
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Optimize { input, output, opt } => {
            let r = process_file(&input, output.as_deref(), &opt.to_options())?;
            println!("\nfactorize-img");
            println!("  in : {:<24} {}", input.display(), human(r.in_size));
            println!(
                "  out: {:<24} {}  ({}x{})",
                r.out.display(),
                human(r.out_size),
                r.width,
                r.height
            );
            println!("  ── {:.1}% smaller   in {:.0}ms ⚡\n", r.saved, r.ms);
        }
        Cmd::Watch { dir, opt } => {
            // .min.* 제외 = 최적화 결과가 다시 이벤트를 일으키는 루프 차단
            let ignored = FsWatcherIgnored::Path("**/*.min.*".to_string());
            let mut watcher = FsWatcher::new(FsWatcherOptions::default(), ignored);

            println!("👀  watching {}  ... (Ctrl+C 로 종료)", dir.display());
            println!("    이미지를 저장/추가하면 자동 최적화됩니다.\n");

            watcher
                .watch(vec![dir], Box::new(NoopAggregate), Box::new(AutoOptimizer { opts: opt.to_options() }))
                .await
                .map_err(|e| anyhow::anyhow!("watch 실패: {e}"))?;

            // watch()는 이벤트 루프를 spawn하고 즉시 반환하므로(napi host 기준 설계) 직접 park —
            // 안 그러면 런타임 종료로 watcher task가 죽음
            tokio::signal::ctrl_c().await.ok();
            println!("\n👋  stopping watcher");
            watcher.close().await;
        }
    }
    Ok(())
}
