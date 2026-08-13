use crate::contract::OrderResponse;
use crate::domain::Order;

pub fn response(order: &Order) -> OrderResponse {
    OrderResponse { id: order.id }
}
