#![deny(unsafe_code)]
#![warn(missing_docs)]

#![allow(mismatched_lifetime_syntaxes)]

//! `textslabs` is an experimental high level text library, with the goal to allow any winit/wgpu program to have full-featured text and text editing with minimal integration effort.
//! 
//! 
//! # Usage
//! 
//! ```no_run
//! # use textslabs::*;
//! // Create the Text struct and the Text renderer:
//! let mut text = Text::new();
//! # let device: wgpu::Device = unimplemented!();
//! # let queue: wgpu::Queue = unimplemented!();
//! # let surface_config: wgpu::SurfaceConfiguration = unimplemented!();
//! let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);
//! // Text manages collections of text boxes and styles.
//! // TextRenderer holds the state needed to render the text on the gpu.
//! 
//! // Add text boxes and get handles:
//! let handle = text.add_text_box("Hello", (10.0, 10.0), (200.0, 50.0), 0.0);
//! let edit_handle = text.add_text_edit("Type here".to_string(), (10.0, 70.0), (200.0, 30.0), 0.0);
//! 
//! // Use handles to access and modify the boxes:
//! text.get_text_edit_mut(&edit_handle).raw_text_mut().push_str("... World");
//! 
//! // Manually remove text boxes when needed:
//! text.remove_text_box(handle);
//! 
//! // In winit's window_event callback, pass the event to Text:
//! # let event: winit::event::WindowEvent = unimplemented!();
//! # let window: winit::window::Window = unimplemented!();
//! text.handle_event(&event, &window);
//! 
//! // Do shaping, layout, rasterization, etc. to prepare the text to be rendered:
//! text.prepare_all(&mut text_renderer);
//! // Load the data on the gpu:
//! text_renderer.load_to_gpu(&device, &queue);
//! // Render the text as part of a wgpu render pass:
//! # let render_pass: wgpu::RenderPass<'_> = unimplemented!();
//! text_renderer.render(&mut render_pass);
//! ```
//! 
//! See the `minimal.rs` or `full.rs` examples in the repository to see a more complete example, including the `winit` and `wgpu` boilerplate.
//! 
//! # Handles
//! 
//! The library is imperative with a handle-based system.
//! 
//! Creating a text box returns a handle that can be used to access it afterwards.
//! 
//! Handles can't be `Clone`d or constructed manually, and removing a text box with [`Text::remove_text_box()`] consumes the handle. So a handle is a unique reference that can never be "dangling".
//! 
//! This interface is ideal for retained-mode GUI libraries, but declarative GUI libraries that diff their node trees can still use the imperative interface, calling the `Text::remove_*` functions when the nodes holding the handles are removed.
//! 
//! [`Text`] uses slotmaps internally, so `get_text_box_mut()` and all similar functions are basically as fast as an array lookup. There is no hashing involved.
//! 
//! # Advanced Usage
//! 
//! ## Accessibility
//! 
//! This library supports accessibility, but integrating it requires a bit more coordination with `winit` and with the GUI code outside of this library. In particular, `textslabs` doesn't have any concept of a tree. See the `accessibility.rs` example in the repository for a basic example.
//! 
//! ## Interaction
//! 
//! Text boxes and text edit boxes are fully interactive. In simple situations, this requires a single function call: [`Text::handle_event()`]. This function takes a `winit::WindowEvent` and updates all the text boxes accordingly.
//! 
//! As great as this sounds, in some cases text boxes can be occluded by other objects, such as an opaque panel. In this case, handling a mouse click event requires information that the [`Text`] struct doesn't have, so the integration needs to be a bit more complex. The process is this:
//! 
//! - Run [`Text::find_topmost_text_box()`] to find out which text box *would* have received the event, if it wasn't for other objects.
//! - Run some custom code to find out which other object *would* have received the event, if it wasn't for text boxes.
//! - Compare the depth of the two candidates. For the text box, use [`Text::get_text_box_depth()`].
//! - If the text box is on top, call [`Text::handle_event_with_topmost()`] with `topmost_text_box = Some(topmost_text_box)`, which will handle the event normally (but skip looking for the topmost box again).
//! - If the text box, is occluded, call [`Text::handle_event_with_topmost()`] with `topmost_text_box = None`.
//! 
//! For any `winit::WindowEvent` other than a `winit::WindowEvent::MouseInput` or a `winit::WindowEvent::MouseWheel`, this process can be skipped, and you can just call [`Text::handle_event()`] normallyw.
//! 
//! The `occlusion.rs` example shows how this works.
//! 
//! ## Declarative Visibility
//! 
//! There is an optional declarative interface for hiding text boxes:
//! 
//! ```no_run
//! # use textslabs::*;
//! # let mut text = Text::new();
//! // Each frame, advance an internal frame counter,
//! // and implicitly mark all text boxes as "outdated"
//! text.advance_frame_and_hide_boxes();
//! 
//! # struct Node { text_box_handle: TextBoxHandle }
//! # let current_nodes: Vec<Node> = Vec::new();
//! // "Refresh" only the nodes that should remain visible
//! for node in current_nodes {
//!     text.refresh_text_box(&node.text_box_handle);
//! }
//! 
//! // Text boxes that were not refreshed will be remain hidden,
//! // and they will be skipped when rendering or handling events.
//! ```
//! 
//! This library was written for use in Keru, which is a declarative library that diffs node trees, so it uses imperative-mode calls to remove widgets. However, it uses the declarative interface for hiding text boxes that need to be kept hidden in the background.
//! 
//! # Open Issues
//! 
//! There are two main open issues in the design of the library:
//! 
//! - All text boxes are rendered in a single draw call. The `TextRenderer` supports using a depth buffer, but that's not enough to get correct results when many semitransparent elements overlap. The only way to solve this problem fully is to draw elements in order. Doing this while keeping the integration simple enough is probably quite hard.
//! 
//! - the math for scrolling and smooth scrolling animations in overflowing text edit boxes is hardcoded in the library. This means that a GUI library using Textslabs might have inconsistent scrolling behavior between the Textslabs text edit boxes and the GUI library's generic scrollable containers.


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

#[cfg(feature = "accessibility")]
mod accessibility;
#[cfg(feature = "accessibility")]
pub use accessibility::*;

pub use parley::TextStyle as ParleyTextStyle;

/// Text style.
/// 
/// To use it, first add a `TextStyle2` into a [`Text`] with [`Text::add_style()`], and get a [`StyleHandle`] back. Then, use [`TextBoxMut::set_style()`] to make a text box use the style.
pub type TextStyle2 = ParleyTextStyle<'static, ColorBrush>;

/// Style configuration for text edit boxes.
/// 
/// Contains color settings that are specific to text edit behavior (disabled/placeholder states).
#[derive(Clone, Debug, PartialEq)]
pub struct TextEditStyle {
    /// Color to use when text is disabled
    pub disabled_text_color: ColorBrush,
    /// Color to use for placeholder text
    pub placeholder_text_color: ColorBrush,
}

impl Default for TextEditStyle {
    fn default() -> Self {
        Self {
            disabled_text_color: ColorBrush([128, 128, 128, 255]), // Gray
            placeholder_text_color: ColorBrush([160, 160, 160, 255]), // Lighter gray
        }
    }
}

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

pub use parley::{FontWeight, FontStyle, LineHeight, FontStack, Alignment, AlignmentOptions, OverflowWrap};
