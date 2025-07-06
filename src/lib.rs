//! Parley2 is an experimental high level text library based on Parley.
//! 
//! The goal is to allow any winit/wgpu program to have full-featured text and text editing with minimal integration effort.
//! 
//! Most GUI programs or toolkits, games, etc. don't have any advanced requirements for text: they just want basic text boxes that "work the same way as everywhere else" (browsers, native operating system GUIs, etc.).
//! 
//! If all that is available is a relatively low level library such as Parley, all these projects will have to do a large amount of repeated work, needlessly raising the barrier of entry to GUI programming.
//! 
//! # Limitations
//! 
//! This library is not ready for use. It currently does not reach its own goal of "full featured text".
//! 
//! - Accessibility is supported in Parley itself but not in Parley2, because of my personal lack of familiarity with the subject.
//! 
//! - Parley2 currently uses the built-in Swash CPU rasterizer and a basic homemade atlas renderer to actually show the text on screen. The performance is acceptable but it is not as good as it could be. There is also a questionable bit of unsafe code to circumvent some performance problems in Swash. This might eventually be fixed by switching to the new "Vello Hybrid" renderer.
//! 
//! - Parley itself has some limitations, but they will be probably fixed soon.
//! 
//! 
//! # Usage
//! 
//! See the `basic.rs` example in the repository to see how the library is used. 
//! 
//! This library has a handle-based interface: when adding a text box with [`Text::add_text_box`], a [`TextBoxHandle`] is returned.
//! 
//! Handles can't be `Clone`d or constructed manually, so each handle is effectively a unique reference to the corresponding text box. It can never be "dangling".
//! 
//! To remove the associated text box, you must remember to call [`Text::remove_text_box`]. This consumes the handle. For example, if every text box is associated to a "widget" in a GUI library, the widget struct will hold a [`TextBoxHandle`]. Then, when the widget is removed, you must call [`Text::remove_text_box`] on its [`TextBoxHandle`].
//! 
//! 
//! 

mod setup;
pub use setup::*;

mod text_renderer;
pub use text_renderer::*;

mod text;
pub use text::*;

mod text_box;
pub use text_box::*;

mod text_edit;
pub use text_edit::*;

pub use parley::{FontWeight, FontStyle, LineHeight, FontStack, TextStyle as ParleyTextStyle};

/// Text style.
/// 
/// To use it, first add a `TextStyle` into a [`Text`] with [`Text::add_style`], and get a [`StyleHandle`] back. Then, use [`TextBox::set_style`] to make a text box use the style.
pub type TextStyle2 = ParleyTextStyle<'static, ColorBrush>;

use bytemuck::{Pod, Zeroable};
use etagere::euclid::{Size2D, UnknownUnit};
use etagere::{size2, Allocation, BucketedAtlasAllocator};
use lru::LruCache;
use rustc_hash::FxHasher;
use swash::zeno::{Format, Vector};

use wgpu::*;

use image::{GrayImage, Luma, Rgba, RgbaImage};
use parley::{
    Glyph, GlyphRun,
    Layout, PositionedLayoutItem,
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
