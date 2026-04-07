use std::fmt::Debug;

use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;

/// 감시에서 제외할 경로 패턴
/// rspack의 FsWatcherIgnored와 동일한 구조
#[derive(Default)]
pub enum FsWatcherIgnored {
    #[default]
    None,
    /// 단일 glob 패턴: "node_modules"
    Path(String),
    /// 복수 glob 패턴: ["node_modules", ".git", "dist"]
    Paths(Vec<String>),
    /// 정규식: /\.git|node_modules/
    Regex(Regex),
}

impl Debug for FsWatcherIgnored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsWatcherIgnored::None => write!(f, "None"),
            FsWatcherIgnored::Path(s) => write!(f, "Path({s})"),
            FsWatcherIgnored::Paths(s) => write!(f, "Paths({s:?})"),
            FsWatcherIgnored::Regex(r) => write!(f, "Regex({r})"),
        }
    }
}

impl FsWatcherIgnored {
    /// 주어진 경로가 무시 대상인지 확인
    pub fn should_ignore(&self, path: &str) -> bool {
        // Windows 호환: 백슬래시를 슬래시로 변환
        let normalized = path.replace('\\', "/");

        match self {
            FsWatcherIgnored::None => false,
            FsWatcherIgnored::Path(pattern) => {
                match build_glob_set(&[pattern.as_str()]) {
                    Some(set) => set.is_match(&normalized),
                    None => normalized.contains(pattern.as_str()),
                }
            }
            FsWatcherIgnored::Paths(patterns) => {
                let refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
                match build_glob_set(&refs) {
                    Some(set) => set.is_match(&normalized),
                    None => patterns.iter().any(|p| normalized.contains(p.as_str())),
                }
            }
            FsWatcherIgnored::Regex(regex) => regex.is_match(&normalized),
        }
    }
}

fn build_glob_set(patterns: &[&str]) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        // "**/{pattern}/**" 형태로 경로 어디든 매칭
        let full = format!("**/{pattern}/**");
        if let Ok(glob) = Glob::new(&full) {
            builder.add(glob);
        }
        // 원본 패턴도 추가 (유저가 직접 glob을 줄 수 있으니)
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_ignores_nothing() {
        let ignored = FsWatcherIgnored::None;
        assert!(!ignored.should_ignore("/src/index.ts"));
    }

    #[test]
    fn test_single_path() {
        let ignored = FsWatcherIgnored::Path("node_modules".to_string());
        assert!(ignored.should_ignore("/project/node_modules/lodash/index.js"));
        assert!(!ignored.should_ignore("/project/src/index.ts"));
    }

    #[test]
    fn test_multiple_paths() {
        let ignored = FsWatcherIgnored::Paths(vec![
            "node_modules".to_string(),
            ".git".to_string(),
            "dist".to_string(),
        ]);
        assert!(ignored.should_ignore("/project/node_modules/lodash/index.js"));
        assert!(ignored.should_ignore("/project/.git/HEAD"));
        assert!(ignored.should_ignore("/project/dist/bundle.js"));
        assert!(!ignored.should_ignore("/project/src/index.ts"));
    }

    #[test]
    fn test_regex() {
        let regex = Regex::new(r"node_modules|\.git").unwrap();
        let ignored = FsWatcherIgnored::Regex(regex);
        assert!(ignored.should_ignore("/project/node_modules/lodash/index.js"));
        assert!(ignored.should_ignore("/project/.git/HEAD"));
        assert!(!ignored.should_ignore("/project/src/index.ts"));
    }
}
