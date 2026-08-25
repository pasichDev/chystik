//! chystik-core: filesystem scanner, category rules and severity engine.
//!
//! Module ownership (v0.2):
//! - `model` — API contract (orchestrator-owned)
//! - `rules/*` — one domain module per owner; see `rules` docs
//! - `scanner`, `guard`, `severity`, `report` — core engine
//! - `disks` — mounted-volume discovery (capacity header, multi-disk scan)
//! - `advisories` — system space Chystik reports but never deletes
//! - `cleaner` — the deletion flow, behind a `Remover` so CI can test it
//! - `blockdev` — attached drives from sysfs, mounted or not
//! - `privacy` — traces of what you did, measured by what they reveal

pub mod advisories;
pub mod app;
pub mod blockdev;
pub mod cleaner;
pub mod config;
pub mod disks;
pub mod guard;
pub mod model;
pub mod platform;
pub mod privacy;
pub mod report;
pub mod rules;
pub mod scanner;
pub mod severity;

pub use disks::DiskInfo;
pub use model::{Category, ChystikError, Finding, ScanProgress, Severity};
