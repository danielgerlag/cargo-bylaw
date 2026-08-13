use shop_api::{create_order, order_response};
use shop_contract::CreateOrderRequest;
use shop_persistence::{InMemoryOrderRepository, OrderRepository};

fn main() {
    let order = create_order(
        42,
        CreateOrderRequest {
            customer_name: "Ada".to_owned(),
        },
    );
    let mut repository = InMemoryOrderRepository::default();
    repository.save(order.clone());
    println!("{:?}", order_response(&order));
}
