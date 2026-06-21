//! 이미지 최적화 코어 — 입출력이 바이트(&[u8] → Vec<u8>)인 순수 함수 optimize

use anyhow::{Context, Result};
use image::imageops::FilterType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutFormat {
    Jpeg,
    Png,
    WebP,
}

impl OutFormat {
    pub fn ext(self) -> &'static str {
        match self {
            OutFormat::Jpeg => "jpg",
            OutFormat::Png => "png",
            OutFormat::WebP => "webp",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OptimizeOptions {
    pub max_width: Option<u32>,
    /// JPEG 품질 1~100 (다른 포맷은 무시)
    pub quality: u8,
    pub format: OutFormat,
}

#[derive(Debug)]
pub struct OptimizeResult {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn optimize(input: &[u8], opts: &OptimizeOptions) -> Result<OptimizeResult> {
    let img = image::load_from_memory(input).context("이미지 디코드 실패 (지원 안 하는 포맷?)")?;

    let img = match opts.max_width {
        Some(w) if img.width() > w => {
            let h = (u64::from(img.height()) * u64::from(w) / u64::from(img.width())) as u32;
            img.resize(w, h.max(1), FilterType::Lanczos3)
        }
        _ => img,
    };
    let (width, height) = (img.width(), img.height());

    let mut bytes: Vec<u8> = Vec::new();
    match opts.format {
        OutFormat::Jpeg => {
            // JPEG은 알파가 없으니 RGB8로
            let mut enc =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, opts.quality);
            enc.encode_image(&img.to_rgb8()).context("JPEG 인코드 실패")?;
        }
        OutFormat::Png => {
            img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
                .context("PNG 인코드 실패")?;
        }
        OutFormat::WebP => {
            // image crate WebP 인코더는 무손실만 지원
            img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::WebP)
                .context("WebP 인코드 실패")?;
        }
    }

    Ok(OptimizeResult { bytes, width, height })
}
