//! overflight: a live view of the aircraft flying above you.
//!
//! This crate is the implementation of the `overflight` **binary**, not a
//! supported library. The command-line program is the product: its flags, the
//! config keys and the logbook format are what the 1.0 stability promise
//! covers. The modules below are public only so the binary, the tests and the
//! documentation can share code; their API is not stable and may change in any
//! release, including a patch.
//!
//! If you would like to reuse one of these pieces as a real library, please
//! open an issue and we can carve out a supported interface for it.

#[doc(hidden)]
pub mod app;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod fetcher;
#[doc(hidden)]
pub mod geo;
#[doc(hidden)]
pub mod logbook;
#[doc(hidden)]
pub mod providers;
#[doc(hidden)]
pub mod render;
#[doc(hidden)]
pub mod route;
#[doc(hidden)]
pub mod satellite;
#[doc(hidden)]
pub mod sky;
#[doc(hidden)]
pub mod sun;
#[doc(hidden)]
pub mod track;
