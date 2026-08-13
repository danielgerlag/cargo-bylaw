#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Order {
    pub id: OrderId,
    pub customer_name: String,
}

impl Order {
    pub fn rename_customer(&mut self, name: impl Into<String>) {
        self.customer_name = name.into();
    }
}
