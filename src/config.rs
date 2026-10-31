use std::mem::size_of;

use crate::sample::Sample;

#[derive(Copy, Clone)]
pub struct Config {
    sample_limit_count: usize,
}

impl Config {
    pub fn with_limit_bytes(size: usize) -> Self {
        Self::with_limit_count(size / size_of::<Sample>())
    }

    pub fn with_limit_count(limit: usize) -> Self {
        Self {
            sample_limit_count: limit,
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        Self::with_limit_bytes(1 * 1024 * 1024)
    }

    pub fn max_size_in_bytes(self) -> usize {
        self.sample_limit_count * size_of::<Sample>()
    }

    pub fn max_samples(self) -> usize {
        self.sample_limit_count
    }
}
