pub struct AliasThing;
pub struct Renamed;
pub struct Factory;

pub trait Buildable {
    fn build() -> super::Helper;
}

impl Buildable for Factory {
    fn build() -> super::Helper {
        super::Helper
    }
}

pub fn meaningful() -> usize {
    42
}
