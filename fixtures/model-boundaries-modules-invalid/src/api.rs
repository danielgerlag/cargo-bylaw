use crate::contract::OrderResponse;
use crate::domain::Order;
use crate::persistence::OrderRecord;

pub fn leaked_persistence(record: &OrderRecord) -> OrderResponse {
    let order = Order {
        id: record.id as u64,
    };
    OrderResponse { id: order.id }
}
