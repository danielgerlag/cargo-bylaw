use shop_domain::{Order, OrderId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderRecord {
    pub id: i64,
    pub customer_name: String,
}

impl From<OrderRecord> for Order {
    fn from(record: OrderRecord) -> Self {
        Self {
            id: OrderId(record.id as u64),
            customer_name: record.customer_name,
        }
    }
}

impl From<Order> for OrderRecord {
    fn from(order: Order) -> Self {
        Self {
            id: order.id.0 as i64,
            customer_name: order.customer_name,
        }
    }
}

pub trait OrderRepository {
    fn save(&mut self, order: Order);
    fn find(&self, id: OrderId) -> Option<Order>;
}

#[derive(Default)]
pub struct InMemoryOrderRepository {
    records: Vec<OrderRecord>,
}

impl OrderRepository for InMemoryOrderRepository {
    fn save(&mut self, order: Order) {
        self.records.push(order.into());
    }

    fn find(&self, id: OrderId) -> Option<Order> {
        self.records
            .iter()
            .find(|record| record.id == id.0 as i64)
            .cloned()
            .map(Into::into)
    }
}
