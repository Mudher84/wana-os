//! Wana OS renderer (Phase 8): GBM buffers, an EGL/OpenGL ES context on
//! top of them, and page-flipping the rendered buffers with `wana-drm`.
//! This is layers 2-4 of the native stack: DRM/KMS, then GBM, then EGL, then GLES.

pub mod egl;
pub mod ffi;
pub mod gbm;
pub mod gl;
pub mod scene;
