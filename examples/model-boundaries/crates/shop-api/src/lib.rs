use shop_contract::{CreateOrderRequest, OrderResponse};
use shop_domain::{Order, OrderId};

pub fn create_order(id: u64, request: CreateOrderRequest) -> Order {
    Order {
        id: OrderId(id),
        customer_name: request.customer_name,
    }
}

pub fn order_response(order: &Order) -> OrderResponse {
    OrderResponse {
        id: order.id.0,
        customer_name: order.customer_name.clone(),
    }
}
