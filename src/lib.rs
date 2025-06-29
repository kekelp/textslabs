//! Parley2 is an experimental high level text library based on Parley.
//! 
//! The goal is to allow any winit/wgpu program to have full-featured text and text editing with minimal integration effort.
//! 
//! Most GUI programs or toolkits, games, etc. don't have any advanced requirements for text: they just want basic text boxes that "work the same way as everywhere else" (browsers, native operating system GUIs, etc.).
//! 
//! If all that is available is a low level flexible library such as Parley, all these projects will have to do a large amount of repeated work, needlessly raising the barrier of entry to GUI programming.
//! 
//! # Example
//! 
//! 
//! # Limitations
//! 
//! This library is not ready for use. It currently does not reach its own goal of "full featured text".
//! 
//! - Accessibility is supported in Parley itself but not in Parley2, because of my personal lack of familiarity with the subject.
//! 
//! - Parley2 currently uses the built-in Swash CPU rasterizer and a basic homemade atlas renderer to actually show the text on screen. The performance is acceptable but it is not as good as it could be. There is also a questionable bit of unsafe code to circumvent some performance problems in Swash. This should be fixed soon by switching to the new Vello Hybrid renderer.
//! 
//! - Parley itself has some limitations:
//!     - font selection on Linux is incomplete
//!     - IME area positioning on Linux is wrong
//!     - some advanced shaping cases are apparently non supported coorrectly
//! 

mod setup;
pub use setup::*;

mod text_renderer;
pub use text_renderer::*;

mod text_box;
pub use text_box::*;

mod text_edit;
pub use text_edit::*;

pub use parley::{TextStyle, FontWeight, FontStyle, LineHeight, FontStack};


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
