//! Wana OS Wayland protocol layer (Phase 10, decision 0001).
//!
//! The wire protocol (socket, message encoding, object IDs, fd passing) is
//! handled by libwayland-server, the freedesktop reference implementation,
//! reached through `sys`. The per-protocol `wl_interface` tables are
//! generated at build time from the protocol XML by `scanner`. Everything
//! above the protocol layer (surfaces, focus, rendering) is Wana's code in
//! `wana-compositor`.

pub mod server;
pub mod sys;

#[cfg(test)]
mod scanner;
#[cfg(test)]
mod tests;

/// Generated protocol tables and opcodes: `protocols::wayland` (core) and
/// `protocols::xdg_shell`.
pub mod protocols {
    include!(concat!(env!("OUT_DIR"), "/protocols.rs"));
}

/// Number of interfaces in the generated protocol tables.
pub fn interface_count() -> (usize, usize) {
    (
        protocols::wayland::INTERFACES.len(),
        protocols::xdg_shell::INTERFACES.len(),
    )
}
