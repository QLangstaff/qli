//! Standard exit codes per CLI conventions (clig.dev).

#![allow(dead_code)] // Some codes are reserved for later phases.

pub const SUCCESS: u8 = 0;
pub const ERROR: u8 = 1;
pub const USAGE: u8 = 2;
pub const SIGINT: u8 = 130;
pub const SIGTERM: u8 = 143;
