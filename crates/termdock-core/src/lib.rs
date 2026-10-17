//! Core of Termdock: everything that does not depend on the UI.

pub mod error;
mod forward;
pub mod hostkeys;
pub mod local;
pub mod profile;
pub mod session;
pub mod sftp;
pub mod shells;
pub mod ssh;
pub mod sshconfig;
pub mod store;
pub mod vault;

pub use error::{Error, Result};
