//! Minimal FFI declarations for libharfbuzz. Only what `wana-text` uses is
//! declared, and only API present in both the Buildroot (HarfBuzz 12.3) and
//! older host versions (8.x). Every HarfBuzz object is an opaque handle;
//! the one struct used (`hb_ot_var_axis_info_t`) is part of the stable ABI.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_uint};

#[derive(Debug)]
pub enum hb_blob_t {}
#[derive(Debug)]
pub enum hb_face_t {}
#[derive(Debug)]
pub enum hb_font_t {}

pub type hb_codepoint_t = u32;
pub type hb_ot_name_id_t = c_uint;
pub type hb_bool_t = c_int;

/// `hb_ot_var_axis_info_t` (hb-ot-var.h, since 2.2.0).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct hb_ot_var_axis_info_t {
    pub axis_index: c_uint,
    pub tag: u32,
    pub name_id: hb_ot_name_id_t,
    pub flags: c_uint,
    pub min_value: f32,
    pub default_value: f32,
    pub max_value: f32,
    pub reserved: c_uint,
}

/// OpenType name IDs.
pub const NAME_ID_FAMILY: hb_ot_name_id_t = 1;
pub const NAME_ID_SUBFAMILY: hb_ot_name_id_t = 2;
pub const NAME_ID_VERSION: hb_ot_name_id_t = 5;

#[link(name = "harfbuzz")]
extern "C" {
    pub fn hb_blob_create_from_file_or_fail(path: *const c_char) -> *mut hb_blob_t;
    pub fn hb_blob_get_length(blob: *mut hb_blob_t) -> c_uint;
    pub fn hb_blob_destroy(blob: *mut hb_blob_t);

    pub fn hb_face_create(blob: *mut hb_blob_t, index: c_uint) -> *mut hb_face_t;
    pub fn hb_face_destroy(face: *mut hb_face_t);
    pub fn hb_face_get_glyph_count(face: *const hb_face_t) -> c_uint;
    pub fn hb_face_get_upem(face: *const hb_face_t) -> c_uint;

    pub fn hb_font_create(face: *mut hb_face_t) -> *mut hb_font_t;
    pub fn hb_font_destroy(font: *mut hb_font_t);
    pub fn hb_font_get_nominal_glyph(
        font: *mut hb_font_t,
        unicode: hb_codepoint_t,
        glyph: *mut hb_codepoint_t,
    ) -> hb_bool_t;

    /// `language` NULL (HB_LANGUAGE_INVALID) picks the best entry.
    pub fn hb_ot_name_get_utf8(
        face: *mut hb_face_t,
        name_id: hb_ot_name_id_t,
        language: *const std::ffi::c_void,
        text_size: *mut c_uint,
        text: *mut c_char,
    ) -> c_uint;

    pub fn hb_ot_var_get_axis_count(face: *mut hb_face_t) -> c_uint;
    pub fn hb_ot_var_get_axis_infos(
        face: *mut hb_face_t,
        start_offset: c_uint,
        axes_count: *mut c_uint,
        axes_array: *mut hb_ot_var_axis_info_t,
    ) -> c_uint;
}
