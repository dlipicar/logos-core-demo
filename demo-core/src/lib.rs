//! The Logos Core Demo's logic: a Logos runtime of the app's own, a local
//! module, and a blockchain node imported from a peered runtime. Qt-free; the
//! Slint app, the headless CLI and the Android build share it.

#[allow(dead_code, unused_imports, unused_comparisons, clippy::all)]
pub mod clients;
pub mod demo;
pub mod invite;
pub mod model;

pub use demo::{exe, Demo, DemoEvent, EventSink, Paths, Peer, PendingPairing, NODE};
