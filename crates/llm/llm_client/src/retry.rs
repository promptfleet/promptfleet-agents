#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
}

impl RetryPolicy {
    pub fn never() -> Self {
        Self { max_retries: 0 }
    }
    pub fn standard() -> Self {
        Self { max_retries: 2 }
    }
}
