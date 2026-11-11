//! Gameplay and startup, above the reusable client library.
// Android's exported native entry needs unsafe; other targets forbid it.
#![cfg_attr(not(target_os = "android"), forbid(unsafe_code))]
#![cfg_attr(target_os = "android", deny(unsafe_code))]
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
pub mod android;
extern crate self as horizon_client;
pub use horizon_client_lib::*;
pub mod entry;
pub mod game;
mod horizon_look;
pub mod mod_api;

#[cfg(test)]
mod brewing_speed_load_tests;
#[cfg(test)]
mod living_death_tests;
