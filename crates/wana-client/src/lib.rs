//! Wana OS Wayland client side (Phase 11): the libwayland-client binding
//! (`client`) and memfd-backed wl_shm buffers (`shm`), shared by the shell
//! (`wana-shell`) and the test client (`wana-wl-test`).

pub mod client;
pub mod layer;
pub mod shm;
