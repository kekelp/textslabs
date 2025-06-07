mod setup;
pub use setup::*;

mod text_renderer;
pub use text_renderer::*;

mod text_box;
pub use text_box::*;

mod text_edit;
pub use text_edit::*;

pub use parley::TextStyle;

use bytemuck::{Pod, Zeroable};
use etagere::euclid::{Size2D, UnknownUnit};
use etagere::{size2, Allocation, BucketedAtlasAllocator};
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
    TextureUsages, TextureViewDescriptor,
};
use swash::zeno::Placement;
