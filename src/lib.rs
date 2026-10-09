//! overflight: a live view of the aircraft flying above you.
//!
//! The library holds the reusable pieces — geometry, solar position and
//! rendering — while `main.rs` is a thin CLI around them. Keeping them in a
//! library makes them available to tests and, later, snapshot tests.

pub mod geo;
pub mod render;
pub mod sun;
