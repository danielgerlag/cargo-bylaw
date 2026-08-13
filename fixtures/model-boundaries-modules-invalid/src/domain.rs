use crate::persistence::OrderRecord;

pub struct Order {
    pub id: u64,
}

pub fn leaked_record(order: &Order) -> OrderRecord {
    OrderRecord {
        id: order.id as i64,
    }
}
