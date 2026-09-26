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

// --- shaping (hb-buffer.h, hb-shape.h) ----------------------------------------
#[derive(Debug)]
pub enum hb_buffer_t {}
#[derive(Debug)]
pub enum hb_language_impl_t {}
pub type hb_language_t = *const hb_language_impl_t;

/// `hb_glyph_info_t` (20 bytes; checked against hb-buffer.h).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct hb_glyph_info_t {
    pub codepoint: u32,
    pub mask: u32,
    pub cluster: u32,
    pub var1: u32,
    pub var2: u32,
}

/// `hb_glyph_position_t` (20 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct hb_glyph_position_t {
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub var: u32,
}

pub const HB_DIRECTION_LTR: c_uint = 4;
pub const HB_DIRECTION_RTL: c_uint = 5;
/// Clusters per character, monotone (for cursor positions inside
/// ligatures and around marks).
pub const HB_BUFFER_CLUSTER_LEVEL_MONOTONE_CHARACTERS: c_uint = 1;

#[link(name = "harfbuzz")]
extern "C" {
    pub fn hb_buffer_create() -> *mut hb_buffer_t;
    pub fn hb_buffer_destroy(buf: *mut hb_buffer_t);
    pub fn hb_buffer_add_utf8(
        buf: *mut hb_buffer_t,
        text: *const c_char,
        text_length: c_int,
        item_offset: c_uint,
        item_length: c_int,
    );
    pub fn hb_buffer_set_direction(buf: *mut hb_buffer_t, direction: c_uint);
    pub fn hb_buffer_set_language(buf: *mut hb_buffer_t, language: hb_language_t);
    pub fn hb_buffer_set_cluster_level(buf: *mut hb_buffer_t, level: c_uint);
    pub fn hb_buffer_guess_segment_properties(buf: *mut hb_buffer_t);
    pub fn hb_buffer_get_glyph_infos(
        buf: *mut hb_buffer_t,
        length: *mut c_uint,
    ) -> *mut hb_glyph_info_t;
    pub fn hb_buffer_get_glyph_positions(
        buf: *mut hb_buffer_t,
        length: *mut c_uint,
    ) -> *mut hb_glyph_position_t;
    pub fn hb_language_from_string(s: *const c_char, len: c_int) -> hb_language_t;
    pub fn hb_shape(
        font: *mut hb_font_t,
        buf: *mut hb_buffer_t,
        features: *const std::ffi::c_void,
        num_features: c_uint,
    );
}

// --- libfribidi (UAX #9) --------------------------------------------------------
pub type FriBidiChar = u32;
pub type FriBidiStrIndex = c_int;
pub type FriBidiCharType = u32;
pub type FriBidiBracketType = u32;
pub type FriBidiParType = u32;
pub type FriBidiLevel = i8;

/// Paragraph directions (values checked against fribidi-bidi-types.h).
pub const FRIBIDI_PAR_LTR: FriBidiParType = 0x110;
pub const FRIBIDI_PAR_RTL: FriBidiParType = 0x111;
pub const FRIBIDI_PAR_ON: FriBidiParType = 0x40;

#[link(name = "fribidi")]
extern "C" {
    pub fn fribidi_get_bidi_types(
        str: *const FriBidiChar,
        len: FriBidiStrIndex,
        btypes: *mut FriBidiCharType,
    );
    pub fn fribidi_get_bracket_types(
        str: *const FriBidiChar,
        len: FriBidiStrIndex,
        types: *const FriBidiCharType,
        btypes: *mut FriBidiBracketType,
    );
    /// Returns max level + 1, or 0 on failure.
    pub fn fribidi_get_par_embedding_levels_ex(
        bidi_types: *const FriBidiCharType,
        bracket_types: *const FriBidiBracketType,
        len: FriBidiStrIndex,
        pbase_dir: *mut FriBidiParType,
        embedding_levels: *mut FriBidiLevel,
    ) -> FriBidiLevel;
}
