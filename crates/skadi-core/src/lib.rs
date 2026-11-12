//! Skadi çekirdeği: platform tespiti, paket yöneticileri, uygulama kataloğu ve kurulum planlama.
//!
//! Arayüzler (CLI, TUI, GUI) yalnızca bu crate'in herkese açık API'sini kullanır;
//! işletim sistemine dokunan tüm mantık burada toplanır.

pub mod audit;
pub mod backup;
pub mod care;
pub mod catalog;
pub mod certs;
pub mod dns;
pub mod error;
pub mod exec;
pub mod export;
pub mod hosts;
pub mod i18n;
pub mod icons;
pub mod maintain;
pub mod manager;
pub mod metadata;
pub mod paths;
pub mod permissions;
pub mod plan;
pub mod platform;
pub mod plugin;
pub mod policy;
pub mod profile;
pub mod proxy;
pub mod schedule;
pub mod settings;
pub mod signing;
pub mod snapshot;
pub mod startup;
pub mod system;
pub mod tweak;
pub mod vault;

pub use error::{Error, Result};
pub use i18n::{Lang, Text};
