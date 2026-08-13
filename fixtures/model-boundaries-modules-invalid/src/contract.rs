use crate::domain::Order;

pub struct OrderResponse {
    pub id: u64,
}

pub fn leaked_domain(response: &OrderResponse) -> Order {
    Order { id: response.id }
}
