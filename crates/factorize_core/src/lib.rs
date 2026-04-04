use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

struct CompilerState(Arc<AtomicBool>);

impl CompilerState {
    fn init() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}

impl CompilerState {
    fn running(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn enter(&self) -> CompilerStateGuard {
        self.0.store(true, Ordering::Relaxed);
        CompilerStateGuard(self.0.clone())
    }
}

struct CompilerStateGuard(Arc<AtomicBool>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
