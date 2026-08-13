#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateOrderRequest {
    pub customer_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderResponse {
    pub id: u64,
    pub customer_name: String,
}
