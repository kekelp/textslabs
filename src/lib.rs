mod wgpu_vomit;

mod text_renderer;


use bytemuck::{Pod, Zeroable};
use etagere::euclid::{Size2D, UnknownUnit};
use etagere::{size2, AllocId, Allocation, BucketedAtlasAllocator};
use lru::LruCache;
use rustc_hash::FxHasher;
use swash::zeno::{Format, Vector};

use wgpu::*;

use image::{GrayImage, Luma, Rgba, RgbaImage};
use parley::{
    FontContext, Glyph, GlyphRun,
    Layout, LayoutContext, PositionedLayoutItem,
};
use std::borrow::Cow;
use std::hash::BuildHasherDefault;
use std::mem;
use std::num::NonZeroU64;
use swash::scale::image::{Content, Image};
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::{FontRef, GlyphId};
use wgpu::{
    MultisampleState, Texture, TextureFormat,
    TextureUsages, TextureView, TextureViewDescriptor,
};

pub struct TextRenderer {
    pub text_renderer: ContextlessTextRenderer,
    pub scale_cx: ScaleContext,
}

pub struct ContextlessTextRenderer {
    pub tmp_image: Image,
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<ColorBrush>,

    color_atlas: Atlas<RgbaImage>,
    mask_atlas: Atlas<GrayImage>,
    
    pub bind_group: BindGroup,
    
    pub params: Params,
    pub params_buffer: Buffer,
    pub params_bind_group: BindGroup,

    pub vertex_buffer: Buffer,
    pub vertex_buffer_size: u64,
    pub pipeline: RenderPipeline,
    pub quads: Vec<Quad>,
    pub atlas_size: u32,
}

pub(crate) struct Atlas<ImageType> {
    packer: BucketedAtlasAllocator,
    glyph_cache: LruCache<GlyphKey, Allocation, BuildHasherDefault<FxHasher>>,
    image: ImageType,
    texture: Texture, // the format here has to match the image type...
    texture_view: TextureView,
}

/// Key for building a glyph cache
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GlyphKey {
    /// Font ID
    pub font_id: u64,
    /// Glyph ID
    pub glyph_id: GlyphId,
    /// `f32` bits of font size
    pub font_size_bits: u32,
    /// Binning of fractional X offset
    pub x_bin: SubpixelBin,
    /// Binning of fractional Y offset
    pub y_bin: SubpixelBin,
    // /// [`CacheKeyFlags`]
    // pub flags: CacheKeyFlags,
}


/// Binning of subpixel position for cache optimization
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SubpixelBin {
    Zero,
    One,
    Two,
    Three,
}

impl SubpixelBin {
    pub fn new(pos: f32) -> (i32, Self) {
        let trunc = pos as i32;
        let fract = pos - trunc as f32;

        if pos.is_sign_negative() {
            if fract > -0.125 {
                (trunc, Self::Zero)
            } else if fract > -0.375 {
                (trunc - 1, Self::Three)
            } else if fract > -0.625 {
                (trunc - 1, Self::Two)
            } else if fract > -0.875 {
                (trunc - 1, Self::One)
            } else {
                (trunc - 1, Self::Zero)
            }
        } else {
            #[allow(clippy::collapsible_else_if)]
            if fract < 0.125 {
                (trunc, Self::Zero)
            } else if fract < 0.375 {
                (trunc, Self::One)
            } else if fract < 0.625 {
                (trunc, Self::Two)
            } else if fract < 0.875 {
                (trunc, Self::Three)
            } else {
                (trunc + 1, Self::Zero)
            }
        }
    }

    pub fn as_float(&self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::One => 0.25,
            Self::Two => 0.5,
            Self::Three => 0.75,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Zeroable, Pod)]
pub struct Quad {
    pub pos: [i32; 2],
    pub dim: [u16; 2],
    pub uv: [u16; 2],
    pub color: u32,
    pub depth: f32,
}

/// A glyph as stored in a glyph atlas
pub(crate) struct StoredGlyph {
    quad: Quad,
    alloc_id: AllocId,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorBrush {
    pub color: Rgba<u8>,
}
impl Default for ColorBrush {
    fn default() -> Self {
        Self {
            color: Rgba([0, 0, 0, 255]),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub screen_resolution: Resolution,
    pub _pad: [u32; 2],
}

/// The screen resolution to use when rendering text.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolution {
    /// The width of the screen in pixels.
    pub width: f32,
    /// The height of the screen in pixels.
    pub height: f32,
}