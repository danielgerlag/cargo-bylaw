use std::sync::Arc;

pub async fn value() -> Arc<str> {
    Arc::from(format!("value:{}", dep_lib::VALUE))
}
