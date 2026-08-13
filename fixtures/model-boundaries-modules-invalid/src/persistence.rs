use crate::contract::OrderResponse;

pub struct OrderRecord {
    pub id: i64,
}

pub fn leaked_contract(record: &OrderRecord) -> OrderResponse {
    OrderResponse {
        id: record.id as u64,
    }
}
