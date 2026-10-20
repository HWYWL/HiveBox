//! The application runtime: packages, sandboxes, display lists.
//!
//! ```text
//! .app file ──▶ package.rs ──▶ Metal ─┐
//!                                       ├─▶ vm.rs (wasmi) ──▶ display.rs ──▶ the launcher's widget
//! catalog.rs ──▶ installed ──▶ services ─┘
//! ```
//!
//! The pieces, and what each one is allowed to know:
//!
//! - [`app_abi`] is the contract. It is the only crate both sides of the sandbox depend on, and it
//!   is `no_std` with no dependencies so that a package can link it without linking a runtime.
//! - [`package`] reads a container and its metadata. It executes nothing.
//! - [`catalog`] walks the two directories that may hold packages and answers what is installed.
//! - [`manager`] installs, upgrades and removes, and is the only module that writes to those
//!   directories.
//! - [`services`] is what the host does for an application: time, text metrics, preferences, files.
//! - [`vm`] instantiates a module with a fuel and memory budget and binds the host functions.
//! - [`hosted`] is one application's life: open, event, frame, fail.
//! - `ui` (the `ui` feature) is the iced widget that draws a [`display::DisplayList`].
//!
//! # What is deliberately not here
//!
//! No renderer, no launcher, no store: this crate runs and manages applications, and the thing that
//! draws a tile or a page lives on the other side of the `ui` feature. That is what lets the runtime,
//! its tests and its headless runner be built without a graphics stack — `cargo test -p app-host`
//! never opens a window.
//!
//! # Running a package on a PC
//!
//! ```text
//! cargo run -p app-host -- ../bin/hivebox-app-calculator-v1.0.0.app
//! ```
//!
//! The runner instantiates the package, builds one frame, and prints what the display list holds —
//! the fastest way to answer "does this package run at all" without a board.

pub mod catalog;
pub mod display;
pub mod hosted;
pub mod manager;
pub mod meta;
pub mod package;
pub mod services;
pub mod version;
pub mod vm;

#[cfg(feature = "ui")]
pub mod ui;

pub use catalog::{Broken, Catalog, Installed, Root, Source};
pub use display::{DisplayList, DrawCommand};
pub use hosted::HostedApp;
pub use manager::{install, uninstall, Expected, InstallError};
pub use meta::Meta;
pub use package::{Icon, Package, PackageError};
pub use services::{Keyboard, Services};
pub use version::Version;
pub use vm::{Limits, Vm, VmError};

/// The directories applications are installed in, as the launcher and the store both need them.
pub fn app_roots() -> Vec<Root> {
    Root::standard()
}
