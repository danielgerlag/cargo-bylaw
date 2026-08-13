use crate::domain::Order;

pub struct OrderRecord {
    pub id: i64,
}

impl From<OrderRecord> for Order {
    fn from(record: OrderRecord) -> Self {
        Self {
            id: record.id as u64,
        }
    }
}
