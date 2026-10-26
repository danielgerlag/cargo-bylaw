use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Value {
    async fn value(&self) -> Arc<str>;
}

pub struct Provider;

#[async_trait]
impl Value for Provider {
    async fn value(&self) -> Arc<str> {
        #[cfg(feature = "unresolved")]
        let _ = missing_dependency::VALUE;
        Arc::from(format!("value:{}", dep_lib::VALUE))
    }
}
