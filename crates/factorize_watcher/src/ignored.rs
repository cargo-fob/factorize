use std::fmt::Debug;

use globset::{Glob, GlobSetBuilder};
use regex::Regex;

/// 감시에서 제외할 경로 패턴
/// rspack의 FsWatcherIgnored와 동일한 구조
/// 패턴은 호출하는 쪽이 glob 형태로 넘겨야 함 (e.g. "**/node_modules/**")
#[derive(Default)]
pub enum FsWatcherIgnored {
    #[default]
    None,
    /// 단일 glob 패턴: "**/node_modules/**"
    Path(String),
    /// 복수 glob 패턴: ["**/node_modules/**", "**/.git/**"]
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
    /// rspack과 동일: 패턴을 그대로 매칭만 함
    pub fn should_ignore(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/");

        match self {
            FsWatcherIgnored::None => false,
            FsWatcherIgnored::Path(pattern) => glob_match(pattern, &normalized),
            FsWatcherIgnored::Paths(patterns) => {
                patterns.iter().any(|p| glob_match(p, &normalized))
            }
            FsWatcherIgnored::Regex(regex) => regex.is_match(&normalized),
        }
    }
}

/// globset을 사용한 glob 매칭
fn glob_match(pattern: &str, path: &str) -> bool {
    Glob::new(pattern)
        .and_then(|g| {
            let mut builder = GlobSetBuilder::new();
            builder.add(g);
            builder.build()
        })
        .map(|set| set.is_match(path))
        .unwrap_or(false)
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
        let ignored = FsWatcherIgnored::Path("**/node_modules/**".to_string());
        assert!(ignored.should_ignore("/project/node_modules/lodash/index.js"));
        assert!(!ignored.should_ignore("/project/src/index.ts"));
    }

    #[test]
    fn test_multiple_paths() {
        let ignored = FsWatcherIgnored::Paths(vec![
            "**/node_modules/**".to_string(),
            "**/.git/**".to_string(),
            "**/dist/**".to_string(),
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
