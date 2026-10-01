//! Wana OS DRM/KMS backend.
//!
//! Layer 1 of the native graphics stack (DRM/KMS, then GBM, then EGL, then GLES). It finds the
//! display hardware through sysfs, opens the card, enumerates connectors
//! and modes, picks a mode, sets it on a CRTC and page-flips framebuffers
//! on vblank. Uses the kernel uAPI directly (`sys`), with no libdrm dependency.

pub mod card;
pub mod connector;
pub mod discover;
pub mod mode;
pub mod output;
pub mod sys;

pub use card::{Card, ConnectorInfo, DumbBuffer, FlipEvent, Resources};
pub use connector::Connection;
pub use discover::CardInfo;
pub use mode::Mode;
