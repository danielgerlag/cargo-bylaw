use helper::nested::AliasThing as RenamedType;
pub use helper::reexports::Exported;
use std::collections::HashMap;

macro_rules! local_macro {
    () => {
        helper::nested::meaningful()
    };
}

fn main() {
    let _map: HashMap<String, String> = HashMap::new();
    let _ = itoa_alias::Buffer::new();
    let _ = RenamedType;
    let _ = <helper::nested::Factory as helper::nested::Buildable>::build();
    let _ = local_macro!();
}

#[cfg(test)]
mod tests {
    fn local_binding() {
        let value = 1;
        let _copy = value;
    }
}
