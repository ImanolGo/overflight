//! overflight: a live view of the aircraft flying above you.
//!
//! The library holds the reusable pieces — geometry, solar position, flight
//! data providers, tracking and rendering — while `main.rs` is a thin CLI
//! around them.

pub mod app;
pub mod config;
pub mod fetcher;
pub mod geo;
pub mod providers;
pub mod render;
pub mod satellite;
pub mod sky;
pub mod sun;
pub mod track;
