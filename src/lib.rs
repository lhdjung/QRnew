// SPDX-License-Identifier: MPL-2.0

//! QRnew's interface, as a library so that `tests/interface.rs` can drive it.
//!
//! The binary in `main.rs` opens a window around [`ui::App`]; the tests build
//! the same component headlessly, with no window, no GPU and no compositor.
//! Both get the same component, which is the whole point of the split.

pub mod i18n;
pub mod ui;
