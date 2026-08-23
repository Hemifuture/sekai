pub mod canvas;
pub mod field;
pub mod map;
pub mod spherical;
// Task 2 lands the isolated view before Task 3 routes real build state to it.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod world_loading;
