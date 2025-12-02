use crate::*;

// Content type constants
const CONTENT_TYPE_MASK: u32 = 0;
const CONTENT_TYPE_COLOR: u32 = 1;
const CONTENT_TYPE_DECORATION: u32 = 2;

// Flag bits
const FADE_ENABLED_BIT: u32 = 4;

fn pack_flags(content_type: u32, fade_enabled: bool) -> u32 {
    content_type | if fade_enabled { 1 << FADE_ENABLED_BIT } else { 0 }
}

fn get_content_type(flags: u32) -> u32 {
    flags & 0x0F
}

/// A struct for rendering text and text edit boxes on the GPU.
/// 
/// Uses traditional CPU-size rasterizing and a dynamic glyph atlas on the GPU.
/// 
/// `TextRenderer` supports only one texture format at a time.
pub struct TextRenderer {
    pub(crate) text_renderer: ContextlessTextRenderer,
    pub(crate) scale_cx: ScaleContext,
}

// This split is needed because of partial borrows
pub(crate) struct ContextlessTextRenderer {
    pub frame: u64,
    pub tmp_image: Image,

    pub(crate) glyph_cache: LruCache<GlyphKey, Option<StoredGlyph>, BuildHasherDefault<FxHasher>>,
    pub(crate) last_frame_evicted: u64,
    
    pub(crate) mask_atlas_pages: Vec<AtlasPage<GrayImage>>,
    pub(crate) color_atlas_pages: Vec<AtlasPage<RgbaImage>>,

    pub(crate) quads: Vec<Quad>,
    
    // Combined texture arrays and single bind group
    pub(crate) mask_texture_array: Texture,
    pub(crate) color_texture_array: Texture,
    pub bind_group_layout: BindGroupLayout,
    pub(crate) bind_group: BindGroup,

    pub params: Params,
    pub sampler: Sampler,
    pub params_buffer: Buffer,

    pub pipeline: RenderPipeline,
    pub atlas_size: u32,
    pub z_range_filtering_enabled: bool,
    
    // pub(crate) cached_scaler: Option<CachedScaler>,
    
    pub(crate) vertex_buffer: Buffer,
    pub(crate) needs_gpu_sync: bool,
    pub(crate) needs_texture_array_rebuild: bool,
}

// pub(crate) struct CachedScaler {
//     scaler: Scaler<'static>,
//     font_key: u64,
//     font_size: f32,
// }

pub(crate) struct AtlasPage<ImageType> {
    pub(crate) packer: BucketedAtlasAllocator,
    pub(crate) image: ImageType,
}



impl ContextlessTextRenderer {
    // for now, we're evicting both masks and colors at the same time even if only one spills over
    // separating them would mean that they can't share the same cache and it would make things more complex 
    fn evict_old_glyphs(&mut self) {
        self.last_frame_evicted = self.frame;

        while let Some((_key, value)) = self.glyph_cache.peek_lru() {
            
            if let Some(stored_glyph) = value {
                if stored_glyph.frame == self.frame {
                    break;
                }
                
                let page = stored_glyph.page as usize;
                match stored_glyph.content_type {
                    Content::Mask => self.mask_atlas_pages[page].packer.deallocate(stored_glyph.alloc.id),
                    Content::Color => self.color_atlas_pages[page].packer.deallocate(stored_glyph.alloc.id),
                    Content::SubpixelMask => unreachable!()
                }
            }
            
            self.glyph_cache.pop_lru();
        }
    }

    fn needs_evicting(&self, current_frame: u64) -> bool {
        self.last_frame_evicted != current_frame
    }

    fn add_selection_rect(&mut self, rect: parley::BoundingBox, left: f32, top: f32, color: u32, clip_rect: Option<parley::BoundingBox>) {        
        let left = left as i32;
        let top = top as i32;

        let mut x0 = left + rect.x0 as i32;
        let mut x1 = left + rect.x1 as i32;
        let mut y0 = top + rect.y0 as i32;
        let mut y1 = top + rect.y1 as i32;

        // Apply clipping if clip_rect is provided
        if let Some(clip) = clip_rect {
            let clip_x0 = left + clip.x0 as i32;
            let clip_x1 = left + clip.x1 as i32;
            let clip_y0 = top + clip.y0 as i32;
            let clip_y1 = top + clip.y1 as i32;

            x0 = x0.max(clip_x0);
            x1 = x1.min(clip_x1);
            y0 = y0.max(clip_y0);
            y1 = y1.min(clip_y1);

            // If the rectangle is completely clipped out, don't add it
            if x0 >= x1 || y0 >= y1 {
                return;
            }
        }

        let quad = Quad {
            pos_packed: pack_i32_pair_as_u16(x0, y0),
            clip_rect_packed: [pack_i16_pair(0, 0), pack_i16_pair(32767, 32767)], // No clipping for decorations
            dim_packed: pack_u16_pair((x1 - x0) as u32, (y1 - y0) as u32),
            uv_origin_packed: pack_u16_pair(0, 0),
            color,
            depth: 0.0,
            flags_and_page: pack_flags_and_page(pack_flags(CONTENT_TYPE_DECORATION, false), 0),
        };
        self.quads.push(quad);
    }
}


/// Key for building a glyph cache
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct GlyphKey {
    /// Font ID
    pub font_id: u64,
    /// Glyph ID
    pub glyph_id: GlyphId,
    /// `f32` bits of font size
    pub font_size_bits: u32,
    /// Binning of fractional X offset
    pub x_bin: SubpixelBin::<4>,
    /// Binning of fractional Y offset
    pub y_bin: SubpixelBin::<4>,
    // /// [`CacheKeyFlags`]
    // pub flags: CacheKeyFlags,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct SubpixelBin<const N: u8>(pub u8);

fn quantize<const N: u8>(pos: f32) -> (i32, f32, SubpixelBin::<N>) {
    let trunc = pos as i32;
    let fract = pos - trunc as f32;
    
    let expanded_bin = if fract.is_sign_negative() {
        let abs_fract = fract.abs();
        ((2 * N) as f32 * (1.0 - abs_fract)).floor() as i32
    } else {
        ((2 * N) as f32 * fract).floor() as i32
    };
    
    let (adjusted_trunc, bin) = if expanded_bin >= 2 * N as i32 {
        (trunc + 1, 0)
    } else if expanded_bin <= 0 {
        (trunc, 0)
    } else {
        let compressed_bin = (expanded_bin + 1) / 2;
        (trunc, compressed_bin as u8)
    };
    
    // todo: return the fract rounded to the subpixel bin 
    return (adjusted_trunc, fract, SubpixelBin::<N>(bin))
}

/// The struct corresponding to the gpu-side representation of a text glyph.
#[allow(missing_docs)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Zeroable, Pod)]
pub struct Quad {
    pub clip_rect_packed: [u32; 2],       // 8 bytes - pack i16 pairs into u32s
    pub pos_packed: u32,                  // 4 bytes - pack x,y as u16,u16
    pub dim_packed: u32,                  // 4 bytes - pack width,height as u16,u16  
    pub uv_origin_packed: u32,            // 4 bytes - pack u,v as u16,u16
    pub color: u32,                       // 4 bytes  
    pub depth: f32,                       // 4 bytes
    pub flags_and_page: u32,              // 4 bytes - flags (24 bits) + page_index (8 bits)
}

impl Quad {
    /// Adjust the position by the given delta, returning false if overflow would occur
    pub fn adjust_position(&mut self, delta_x: i32, delta_y: i32) -> bool {
        let (x, y) = unpack_pos_as_i32(self.pos_packed);
        
        let new_x = x - delta_x;  // Assuming the original code subtracts
        let new_y = y - delta_y;
        
        // Check for i16 overflow/underflow.
        // Feels a bit bad to do these checks all the time, but whatever.
        if new_x < i16::MIN as i32 || new_x > i16::MAX as i32 ||
           new_y < i16::MIN as i32 || new_y > i16::MAX as i32 {
            return false; // Overflow would occur, abort
        }
        
        self.pos_packed = pack_i32_pair_as_u16(new_x, new_y);
        true // Success
    }
}

// Helper functions to pack/unpack u16 pairs into u32
fn pack_u16_pair(a: u32, b: u32) -> u32 {
    (a & 0xFFFF) | ((b & 0xFFFF) << 16)
}

// Saturating cast from i32 to i16 (preserves negatives within i16 range)
fn saturating_i16_cast(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

// Pack i32 coordinates as i16 pairs with saturating cast (stored as u16 bit patterns)
fn pack_i32_pair_as_u16(a: i32, b: i32) -> u32 {
    let a_i16 = saturating_i16_cast(a);
    let b_i16 = saturating_i16_cast(b);
    (a_i16 as u16 as u32) | ((b_i16 as u16 as u32) << 16)
}

// Unpack u32 to two i32 coordinates (i16 bit patterns -> i32)
fn unpack_pos_as_i32(packed: u32) -> (i32, i32) {
    let x_u16 = (packed & 0xFFFF) as u16;
    let y_u16 = ((packed >> 16) & 0xFFFF) as u16;
    let x = x_u16 as i16 as i32;
    let y = y_u16 as i16 as i32;
    (x, y)
}

// Helper functions to pack/unpack i16 pairs into u32
fn pack_i16_pair(a: i32, b: i32) -> u32 {
    ((a as u16) as u32) | (((b as u16) as u32) << 16)
}

// Pack flags (24 bits) and page_index (8 bits) into u32
fn pack_flags_and_page(flags: u32, page_index: u32) -> u32 {
    (flags & 0xFFFFFF) | ((page_index & 0xFF) << 24)
}

// Unpack flags from packed field
fn unpack_flags_rust(flags_and_page: u32) -> u32 {
    flags_and_page & 0xFFFFFF
}

// Unpack page_index from packed field  
fn unpack_page_index_rust(flags_and_page: u32) -> u32 {
    (flags_and_page >> 24) & 0xFF
}


fn make_quad(glyph: &GlyphWithContext, stored_glyph: &StoredGlyph, depth: f32) -> Quad {
    let y = glyph.quantized_pos_y - stored_glyph.placement_top as i32;
    let x = glyph.quantized_pos_x + stored_glyph.placement_left as i32;

    let (uv_x, uv_y) = (stored_glyph.alloc.rectangle.min.x, stored_glyph.alloc.rectangle.min.y);
    let (size_x, size_y) = (stored_glyph.size.width, stored_glyph.size.height);

    let (color, flags) = match stored_glyph.content_type {
        Content::Mask => (glyph.color, CONTENT_TYPE_MASK),
        Content::Color => (0xff_ff_ff_ff, CONTENT_TYPE_COLOR),
        Content::SubpixelMask => unreachable!(),
    };
    return Quad {
        pos_packed: pack_i32_pair_as_u16(x, y),
        clip_rect_packed: [pack_i16_pair(0, 0), pack_i16_pair(32767, 32767)], // No clipping (will be set later)
        dim_packed: pack_u16_pair(size_x as u32, size_y as u32),
        uv_origin_packed: pack_u16_pair(uv_x as u32, uv_y as u32),
        color,
        depth,
        flags_and_page: pack_flags_and_page(pack_flags(flags, false), stored_glyph.page as u32),
    };
}

fn clip_quad(quad: Quad, left: f32, top: f32, clip_rect: Option<parley::BoundingBox>, fade: bool) -> Option<Quad> {
    let mut quad = quad;

    if let Some(clip) = clip_rect {
        let left = left as i32;
        let top = top as i32;
        
        let clip_x0 = left + clip.x0 as i32;
        let clip_x1 = left + clip.x1 as i32;
        let clip_y0 = top + clip.y0 as i32;
        let clip_y1 = top + clip.y1 as i32;

        // Set the GPU clip rectangle  
        let clamped_x0 = clip_x0.clamp(i16::MIN as i32, i16::MAX as i32);
        let clamped_y0 = clip_y0.clamp(i16::MIN as i32, i16::MAX as i32);
        let clamped_x1 = clip_x1.clamp(i16::MIN as i32, i16::MAX as i32);
        let clamped_y1 = clip_y1.clamp(i16::MIN as i32, i16::MAX as i32);
        quad.clip_rect_packed = [
            pack_i16_pair(clamped_x0, clamped_y0),
            pack_i16_pair(clamped_x1, clamped_y1)
        ];

        // Extract content type and page from existing packed field
        let existing_flags = unpack_flags_rust(quad.flags_and_page);
        let page_index = unpack_page_index_rust(quad.flags_and_page);
        let content_type = existing_flags & 0x0F;
        
        // Update flags with fade bit, keeping same page
        quad.flags_and_page = pack_flags_and_page(pack_flags(content_type, fade), page_index);
    } else {
        // No clipping - use maximum clip rectangle
        quad.clip_rect_packed = [pack_i16_pair(0, 0), pack_i16_pair(32767, 32767)];
    }
    
    Some(quad)
}

/// A glyph as stored in a glyph atlas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StoredGlyph {
    content_type: Content,
    page: u16,
    frame: u64,
    alloc: Allocation,
    placement_left: i32,
    placement_top: i32,
    size: Size2D<i32, UnknownUnit>,
}
impl StoredGlyph {
    fn create(alloc: &Allocation, placement: &Placement, page: usize, frame: u64, content_type: Content) -> StoredGlyph {
        StoredGlyph {
            content_type,
            page: page as u16,
            frame,
            alloc: alloc.clone(),
            placement_left: placement.left,
            placement_top: placement.top,
            size: placement.size(),
        }
    }
}

/// RGBA color value for text rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorBrush(pub [u8; 4]);
impl Default for ColorBrush {
    fn default() -> Self {
        Self([0, 0, 0, 255])
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct Params {
    /// The width of the screen in pixels.
    pub screen_resolution_width: f32,
    /// The height of the screen in pixels.
    pub screen_resolution_height: f32,
    pub srgb: u32,
    pub _pad: u32,
}

impl TextRenderer {    
    /// Create a new TextRenderer with custom parameters.
    pub fn new_with_params(
        device: &Device,
        _queue: &Queue,
        format: TextureFormat,
        depth_stencil: Option<DepthStencilState>,
        params: TextRendererParams,
    ) -> Self {
        Self {
            scale_cx: ScaleContext::new(),
            text_renderer: ContextlessTextRenderer::new_with_params(device, _queue, format, depth_stencil, params),
        }
    }

    /// Create a new TextRenderer with default parameters.
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self::new_with_params(device, queue, format, None, TextRendererParams::default())
    }

    /// Update the screen resolution for text rendering
    pub fn update_resolution(&mut self, width: f32, height: f32) {
        self.text_renderer.update_resolution(width, height);
    }

    /// Clear all render data for text and decorations from the renderer.
    pub fn clear(&mut self) {
        self.text_renderer.clear();
        self.text_renderer.clear_decorations();
    }

    /// Clear only the render data for decorations, leaving text intact.
    pub fn clear_decorations_only(&mut self) {
        self.text_renderer.clear_decorations();
    }

    /// Prepare an individual parley layout for rendering at the specified position.
    pub fn prepare_layout(&mut self, layout: &Layout<ColorBrush>, left: f32, top: f32, clip_rect: Option<parley::BoundingBox>, fade: bool, depth: f32) {
        self.text_renderer.prepare_layout(layout, &mut self.scale_cx, left, top, clip_rect, fade, depth);
        self.text_renderer.needs_gpu_sync = true;
    }

    /// Prepare a text box layout for rendering with scrolling and clipping support.
    pub fn prepare_text_box_layout(&mut self, text_box: &mut TextBoxInner) {
        if text_box.hidden() {
            return;
        }
        text_box.refresh_layout();
                
        let (left, top) = text_box.position();
        let (left, top) = (left as f32, top as f32);
        let clip_rect = text_box.effective_clip_rect();
        let fade = text_box.fadeout_clipping();

        let content_left = left - text_box.scroll_offset().0;
        let content_top = top - text_box.scroll_offset().1;

        let start_index = self.text_renderer.quads.len();

        self.text_renderer.prepare_layout(&text_box.layout, &mut self.scale_cx, content_left, content_top, clip_rect, fade, text_box.depth);
        self.text_renderer.needs_gpu_sync = true;
        
        // Update quad storage with new ranges
        let scroll_offset = text_box.scroll_offset();
        self.capture_quad_ranges_after(&mut text_box.quad_storage, scroll_offset, start_index);
    }

    /// Prepare a text edit layout for rendering with scrolling and clipping support.
    pub fn prepare_text_edit_layout(&mut self, text_edit: &mut TextEditInner) {
        if text_edit.hidden() {
            return;
        }
        
        text_edit.refresh_layout();

        let (left, top) = text_edit.pos();
        let (left, top) = (left as f32, top as f32);
        let clip_rect = text_edit.text_box.effective_clip_rect();
        let fade = text_edit.fadeout_clipping();

        let content_left = left - text_edit.scroll_offset().0;
        let content_top = top - text_edit.scroll_offset().1;

        let start_index = self.text_renderer.quads.len();

        self.text_renderer.prepare_layout(&text_edit.text_box.layout, &mut self.scale_cx, content_left, content_top, clip_rect, fade, text_edit.text_box.depth);
        self.text_renderer.needs_gpu_sync = true;
        
        // Update quad storage with new ranges
        let scroll_offset = text_edit.scroll_offset();
        self.capture_quad_ranges_after(&mut text_edit.text_box.quad_storage, scroll_offset, start_index);
    }

    /// Prepare decorations (selection and cursor) for a text box.
    pub fn prepare_text_box_decorations(&mut self, text_box: &TextBoxInner, show_cursor: bool) {
        let (left, top) = text_box.position();
        let (left, top) = (left as f32, top as f32);
        let clip_rect = text_box.effective_clip_rect();

        let content_left = left - text_box.scroll_offset().0;
        let content_top = top - text_box.scroll_offset().1;

        let selection_color = 0x33_33_ff_aa;
        let cursor_color = 0xee_ee_ee_ff;

        text_box.selection().geometry_with(&text_box.layout, |rect, _line_i| {
            self.text_renderer.add_selection_rect(rect, content_left, content_top, selection_color, clip_rect);
        });
        
        let show_cursor = show_cursor && text_box.selection().is_collapsed();
        if show_cursor {
            let size = CURSOR_WIDTH;
            let cursor_rect = text_box.selection().focus().geometry(&text_box.layout, size);
            self.text_renderer.add_selection_rect(cursor_rect, content_left, content_top, cursor_color, clip_rect);
        }
        self.text_renderer.needs_gpu_sync = true;
    }

    /// Load the render data to the GPU.
    pub fn load_to_gpu(&mut self, device: &Device, queue: &Queue) {
        self.text_renderer.load_to_gpu(device, queue);
    }

    /// Render all prepared text using the provided render pass.
    pub fn render(&self, pass: &mut RenderPass<'_>) {
        self.text_renderer.render(pass);
    }

    /// Render the prepared text within the specified z-range.
    /// 
    /// This function uses `wgpu`'s push constants, and can only be used if the `TextRenderer` was created with the `enable_z_range_filtering = true` option in [`TextRendererParams`].
    /// 
    /// Note that push constants are a native-only feature that may not be available in some `wgpu` backends. See the `wgpu` documentation for more information.
    /// 
    /// # Panics
    /// 
    /// Panics if the `TextRenderer` was not created with `enable_z_range_filtering = true`.
    pub fn render_z_range(&self, pass: &mut RenderPass<'_>, z_range: [f32; 2]) {
        self.text_renderer.render_z_range(pass, z_range);
    }
    
    /// Capture quad ranges after text rendering and populate QuadStorage
    fn capture_quad_ranges_after(&mut self, quad_storage: &mut QuadStorage, current_offset: (f32, f32), start_index: usize) {
        let end_index = self.text_renderer.quads.len();
        // Store the range if any quads were added
        quad_storage.quad_range = if end_index > start_index {
            Some((start_index, end_index))
        } else {
            None
        };
        
        quad_storage.last_offset = current_offset;
    }

    /// Get the vertex buffer for external rendering
    pub fn vertex_buffer(&self) -> &Buffer {
        &self.text_renderer.vertex_buffer
    }

    /// Get the bind group for external rendering.
    pub fn bind_group(&self) -> BindGroup {
        self.text_renderer.bind_group.clone()
    }

    /// Get the bind group layout for external rendering.
    pub fn bind_group_layout(&self) -> BindGroupLayout {
        self.text_renderer.bind_group_layout.clone()
    }

    /// Get the quads buffer for external rendering
    pub fn quads(&self) -> &[Quad] {
        &self.text_renderer.quads
    }

    /// Get the render pipeline for external rendering
    pub fn pipeline(&self) -> &RenderPipeline {
        &self.text_renderer.pipeline
    }

    /// Get mask texture array for external rendering
    pub fn mask_texture_array(&self) -> &Texture {
        &self.text_renderer.mask_texture_array
    }

    /// Get color texture array for external rendering  
    pub fn color_texture_array(&self) -> &Texture {
        &self.text_renderer.color_texture_array
    }

    /// Get the atlas sampler for external rendering
    pub fn sampler(&self) -> &Sampler {
        &self.text_renderer.sampler
    }
}

const SOURCES: &[Source; 3] = &[
    Source::ColorOutline(0),
    Source::ColorBitmap(StrikeWith::BestFit),
    Source::Outline,
];

impl ContextlessTextRenderer {
    pub fn render(&self, pass: &mut RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

        if self.z_range_filtering_enabled {
            pass.set_push_constants(wgpu::ShaderStages::VERTEX, 0, bytemuck::bytes_of(&[f32::MAX, f32::MIN]));
        }

        // Calculate total instance count
        let total_instances = self.quads.len();

        if total_instances > 0 {
            // Single draw call for all instances
            pass.draw(0..4, 0..(total_instances as u32));
        }
    }

    pub fn load_to_gpu(&mut self, device: &Device, queue: &Queue) {
        if !self.needs_gpu_sync && !self.needs_texture_array_rebuild {
            return;
        }

        // Update uniform buffer
        let bytes: &[u8] = bytemuck::cast_slice(std::slice::from_ref(&self.params));
        queue.write_buffer(&self.params_buffer, 0, bytes);

        // Rebuild texture arrays if needed
        if self.needs_texture_array_rebuild {
            self.rebuild_texture_arrays(device, queue);
            self.needs_texture_array_rebuild = false;
        } else {
            self.update_texture_arrays(queue);
        }

        // Calculate total number of quads
        let required_size = (self.quads.len() * std::mem::size_of::<Quad>()) as u64;
        
        // Grow shared vertex buffer if needed
        if self.vertex_buffer.size() < required_size {
            let min_size = u64::max(required_size, INITIAL_BUFFER_SIZE);
            let growth_size = min_size * 3 / 2;
            let current_growth = self.vertex_buffer.size() * 3 / 2;
            let new_size = u64::max(growth_size, current_growth);
            
            self.vertex_buffer = create_vertex_buffer(device, new_size);
            self.recreate_bind_group(device);
        }

        // Write all quads to vertex buffer
        if !self.quads.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&self.quads);
            queue.write_buffer(&self.vertex_buffer, 0, bytes);
        }

        self.needs_gpu_sync = false;
    }

    pub fn render_z_range(&self, pass: &mut RenderPass<'_>, z_range: [f32; 2]) {
        if !self.z_range_filtering_enabled {
            panic!("Z-range filtering was not enabled when creating this TextRenderer. Set TextRendererParams::enable_z_range_filtering = true");
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_push_constants(wgpu::ShaderStages::VERTEX, 0, bytemuck::bytes_of(&z_range));
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

        // Calculate total instance count
        let total_instances = self.quads.len();

        if total_instances > 0 {
            // Single draw call for all instances
            pass.draw(0..4, 0..(total_instances as u32));
        }
    }

    pub fn update_resolution(&mut self, width: f32, height: f32) {
        self.params.screen_resolution_width = width;
        self.params.screen_resolution_height = height;
    }

    pub fn clear(&mut self) {
        self.frame += 1;
        self.quads.clear();
        self.needs_gpu_sync = true;
    }

    pub fn clear_decorations(&mut self) {
        // Since decorations are now mixed with regular quads, 
        // we need to filter them out by content type
        self.quads.retain(|quad| {
            get_content_type(unpack_flags_rust(quad.flags_and_page)) != CONTENT_TYPE_DECORATION
        });
        self.needs_gpu_sync = true;
    }


    fn prepare_layout(&mut self, layout: &Layout<ColorBrush>, scale_cx: &mut ScaleContext, left: f32, top: f32, clip_rect: Option<parley::BoundingBox>, fade: bool, depth: f32) {
        for line in layout.lines() {
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        self.prepare_glyph_run(&glyph_run, scale_cx, left, top, clip_rect, fade, depth);
                    }
                    PositionedLayoutItem::InlineBox(_inline_box) => {}
                }
            }
        }
    }

    fn prepare_glyph_run(
        &mut self,
        glyph_run: &GlyphRun<'_, ColorBrush>,
        scale_cx: &mut ScaleContext,
        left: f32,
        top: f32,
        clip_rect: Option<parley::BoundingBox>,
        fade: bool,
        depth: f32
    ) {
        let mut run_x = left + glyph_run.offset();
        let run_y = top + glyph_run.baseline();
        let style = glyph_run.style();

        let run = glyph_run.run();

        let font = run.font();
        let font_size = run.font_size();
        let font_ref = FontRef::from_index(font.data.as_ref(), font.index as usize).unwrap();
        let font_key = font.data.id();

        // // Why is creating this struct so slow anyway?
        // // This optimization won't do anything if the font size changes a lot.
        // // It might be over.
        // // todo: feel bad about this
        // let need_new_scaler = self.cached_scaler.as_ref()
        //     .map(|cached| cached.font_key != font_key || cached.font_size != font_size)
        //     .unwrap_or(true);

        // if need_new_scaler {
        //     let scaler = scale_cx
        //         .builder(font_ref)
        //         .size(font_size)
        //         .hint(true)
        //         .normalized_coords(run.normalized_coords())
        //         .build();
            
        //     self.cached_scaler = Some(CachedScaler {
        //         // SAFETY: I have no idea, but we reuse a scaler only if the font_key is the same, which should mean that the font data is still valid.
        //         scaler: unsafe { std::mem::transmute(scaler) },
        //         font_key,
        //         font_size,
        //     });
        // }

        let mut scaler = scale_cx
            .builder(font_ref)
            .size(font_size)
            .hint(true)
            .normalized_coords(run.normalized_coords())
            .build();

        for glyph in glyph_run.glyphs() {
            let glyph_ctx = GlyphWithContext::new(glyph, run_x, run_y, font_key, font_size, style.brush);

            if let Some(stored_glyph) = self.glyph_cache.get(&glyph_ctx.key()) {
                if let Some(stored_glyph) = stored_glyph {
                    let quad = make_quad(&glyph_ctx, stored_glyph, depth);
                    if let Some(clipped_quad) = clip_quad(quad, left, top, clip_rect, fade) {
                        self.quads.push(clipped_quad);
                    }
                }
            } else {
                if let Some((quad, _stored_glyph)) = self.prepare_glyph(&glyph_ctx, &mut scaler, depth) {
                    if let Some(clipped_quad) = clip_quad(quad, left, top, clip_rect, fade) {
                        self.quads.push(clipped_quad);
                    }
                }
            }

            run_x += glyph.advance;
        }

        // Draw decorations: underline & strikethrough
        // let style = glyph_run.style();
        // let run_metrics = run.metrics();
        // if let Some(decoration) = &style.underline {
        //     let offset = decoration.offset.unwrap_or(run_metrics.underline_offset);
        //     let size = decoration.size.unwrap_or(run_metrics.underline_size);
        //     render_decoration(img, glyph_run, decoration.brush, offset, size, padding);
        // }
        // if let Some(decoration) = &style.strikethrough {
        //     let offset = decoration
        //         .offset
        //         .unwrap_or(run_metrics.strikethrough_offset);
        //     let size = decoration.size.unwrap_or(run_metrics.strikethrough_size);
        //     render_decoration(img, glyph_run, decoration.brush, offset, size, padding);
        // }
    }

    fn copy_glyph_to_atlas(&mut self, size: Size2D<i32, UnknownUnit>, alloc: &Allocation, page: usize, content_type: Content) {
        for y in 0..size.height as i32 {
            let src_start = (y as usize) * (size.width as usize);
            let src_slice =
                &self.tmp_image.data[src_start..(src_start + size.width as usize)];

            let dst_y = (alloc.rectangle.min.y + y) as u32;
            let dst_x = alloc.rectangle.min.x as u32;

            match content_type {
                Content::Mask => {
                    let layout = self.mask_atlas_pages[page].image.as_flat_samples().layout;
                    let mut samples = self.mask_atlas_pages[page].image.as_flat_samples_mut();
                    let samples = samples.as_mut_slice();
                    let dst_start =
                    (dst_y as usize) * layout.height_stride + (dst_x as usize) * layout.width_stride;
    
                samples[dst_start..(dst_start + size.width as usize)].copy_from_slice(src_slice);
                },
                Content::Color => {
                    // todo: rewrite this with a cool copy_from_slice
                    let layout = self.color_atlas_pages[page].image.as_flat_samples().layout;
                    let mut samples = self.color_atlas_pages[page].image.as_flat_samples_mut();
                    let samples = samples.as_mut_slice();
                    
                    // For RGBA, each pixel is 4 bytes
                    for x in 0..size.width as usize {
                        let src_idx = src_start + x;
                        let dst_idx = (dst_y as usize) * layout.height_stride + (dst_x as usize + x) * layout.width_stride;
                        
                        // Copy all 4 channels
                        for c in 0..4 {
                            samples[dst_idx + c] = self.tmp_image.data[src_idx * 4 + c];
                        }
                    }
                },
                Content::SubpixelMask => unreachable!(),
            };
        }
    }

    /// Render a glyph into the `self.tmp_swash_image` buffer
    // this is going to have to return the Content (color/mask) as well
    fn _render_glyph(&mut self, glyph: &GlyphWithContext, scaler: &mut Scaler) -> (Content, Placement) {
        self.tmp_image.clear();
        Render::new(SOURCES)
            .format(Format::Alpha)
            .offset(glyph.frac_offset())
            .render_into(scaler, glyph.glyph.id as u16, &mut self.tmp_image);
        return (self.tmp_image.content, self.tmp_image.placement);
    }

    // /// Helper method to prepare glyph using the cached scaler
    // fn prepare_glyph_with_cached_scaler(&mut self, glyph: &GlyphWithContext) -> Option<(Quad, StoredGlyph)> {
    //     if self.cached_scaler.is_none() {
    //         return None;
    //     }

    //     let (content, placement) = self.render_glyph_with_cached_scaler(&glyph)?;
    //     let size = placement.size();
        
    //     // For some glyphs there's no image to store, like spaces.
    //     if size.is_empty() {
    //         self.glyph_cache.push(glyph.key(), None);
    //         return None;
    //     }
        
    //     let n_pages = match content {
    //         Content::Mask => self.mask_atlas_pages.len(),
    //         Content::Color => self.color_atlas_pages.len(),
    //         Content::SubpixelMask => unreachable!(),
    //     };
        
    //     // Try to allocate on existing pages
    //     for page in 0..n_pages {
    //         if let Some(alloc) = self.pack_rectangle(size, content, page) {
    //             return self.store_glyph(glyph, size, &alloc, page, &placement, content);
    //         }
            
    //         // Try evicting glyphs from previous frames and retry
    //         if self.needs_evicting(self.frame) {
    //             self.evict_old_glyphs();
                
    //             if let Some(alloc) = self.pack_rectangle(size, content, page) {
    //                 return self.store_glyph(glyph, size, &alloc, page, &placement, content);
    //             }
    //         }
    //     }
        
    //     // Create a new page and try to allocate there
    //     let new_page: usize = self.make_new_page(content);
    //     if let Some(alloc) = self.pack_rectangle(size, content, new_page) {
    //         return self.store_glyph(glyph, size, &alloc, new_page, &placement, content);
    //     }
        
    //     // Glyph is too large to fit even in a new empty page
    //     self.glyph_cache.push(glyph.key(), None);
    //     None
    // }

    // /// Render a glyph using the cached scaler
    // fn render_glyph_with_cached_scaler(&mut self, glyph: &GlyphWithContext) -> Option<(Content, Placement)> {
    //     if let Some(cached) = &mut self.cached_scaler {
    //         self.tmp_image.clear();
    //         Render::new(SOURCES)
    //             .format(Format::Alpha)
    //             .offset(glyph.frac_offset())
    //             .render_into(&mut cached.scaler, glyph.glyph.id, &mut self.tmp_image);
    //         Some((self.tmp_image.content, self.tmp_image.placement))
    //     } else {
    //         None
    //     }
    // }

    /// Rasterizes the glyph in a texture atlas and returns a Quad that can be used to render it, or None if the glyph was just empty (like a space).
    fn prepare_glyph(&mut self, glyph: &GlyphWithContext, scaler: &mut Scaler, depth: f32) -> Option<(Quad, StoredGlyph)> {
        let (content, placement) = self._render_glyph(&glyph, scaler);
        let size = placement.size();
        
        // For some glyphs there's no image to store, like spaces.
        if size.is_empty() {
            self.glyph_cache.push(glyph.key(), None);
            return None;
        }
        
        let n_pages = match content {
            Content::Mask => self.mask_atlas_pages.len(),
            Content::Color => self.color_atlas_pages.len(),
            Content::SubpixelMask => unreachable!(),
        };
        // Try to allocate on existing pages
        for page in 0..n_pages {
            if let Some(alloc) = self.pack_rectangle(size, content, page) {
                return self.store_glyph(glyph, size, &alloc, page, &placement, content, depth);
            }
            
            // Try evicting glyphs from previous frames and retry
            if self.needs_evicting(self.frame) {
                self.evict_old_glyphs();
                
                if let Some(alloc) = self.pack_rectangle(size, content, page) {
                    return self.store_glyph(glyph, size, &alloc, page, &placement, content, depth);
                }
            }
        }
        
        // Create a new page and try to allocate there
        let new_page: usize = self.make_new_page(content);
        if let Some(alloc) = self.pack_rectangle(size, content, new_page) {
            return self.store_glyph(glyph, size, &alloc, new_page, &placement, content, depth);
        }
        
        // Glyph is too large to fit even in a new empty page. It's time to give up.
        // todo: should probably try to catch these earlier by checking for unreasonable font sizes
        // todo2: technically, we could split the huge glyph across multiple pages, or render it on the surface directly.
        self.glyph_cache.push(glyph.key(), None);
        return None;
    }
    
    // Helper method to store glyph once allocation is successful
    // todo: don't carry around `size`, alloc probably has the same data
    fn store_glyph(&mut self, 
            glyph: &GlyphWithContext,
            size: Size2D<i32, UnknownUnit>                            , 
            alloc: &Allocation, 
            page: usize, 
            placement: &Placement,
            content_type: Content,
            depth: f32,
        ) -> Option<(Quad, StoredGlyph)> {
        self.copy_glyph_to_atlas(size, alloc, page, content_type);
        let stored_glyph = StoredGlyph::create(alloc, placement, page, self.frame, content_type);
        self.glyph_cache.push(glyph.key(), Some(stored_glyph));
        let quad = make_quad(glyph, &stored_glyph, depth);
        Some((quad, stored_glyph))
    }

    fn pack_rectangle(&mut self, size: Size2D<i32, UnknownUnit>, content_type: Content, page: usize) -> Option<Allocation> {
        match content_type {
            Content::Mask => self.mask_atlas_pages[page].packer.allocate(size),
            Content::Color => self.color_atlas_pages[page].packer.allocate(size),
            Content::SubpixelMask => unreachable!(),
        }
    }

    fn make_new_page(&mut self, content_type: Content) -> usize {
        let atlas_size = self.atlas_size;

        match content_type {
            Content::Mask => {
                self.mask_atlas_pages.push(AtlasPage {
                    image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
                    packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
                });
                self.needs_texture_array_rebuild = true;
                return self.mask_atlas_pages.len() - 1;
            },
            Content::Color => {
                self.color_atlas_pages.push(AtlasPage {
                    image: RgbaImage::from_pixel(atlas_size, atlas_size, Rgba([0, 0, 0, 0])),
                    packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
                });
                self.needs_texture_array_rebuild = true;
                return self.color_atlas_pages.len() - 1;
            },
            Content::SubpixelMask => unreachable!()
        };
    }
}

/// A glyph with the context in which it is being drawn 
struct GlyphWithContext {
    glyph: Glyph,
    color: u32,
    font_key: u64,
    font_size: f32,
    quantized_pos_x: i32,
    quantized_pos_y: i32,
    frac_pos_x: f32,
    frac_pos_y: f32,
    subpixel_bin_x: SubpixelBin<4>,
    subpixel_bin_y: SubpixelBin<4>,
}

impl GlyphWithContext {
    fn new(glyph: Glyph, run_x: f32, run_y: f32, font_key: u64, font_size: f32, color: ColorBrush) -> Self {
        let glyph_x = (run_x).round() + glyph.x;
        let glyph_y = (run_y).round() - glyph.y;

        let (quantized_pos_x, frac_pos_x, subpixel_bin_x) = quantize(glyph_x);
        let (quantized_pos_y, frac_pos_y, subpixel_bin_y) = quantize(glyph_y);

        let color = 
          ((color.0[0] as u32) << 24)
        + ((color.0[1] as u32) << 16)
        + ((color.0[2] as u32) << 8)
        + ((color.0[3] as u32) << 0);

        Self { glyph, color, font_key, font_size, quantized_pos_x, quantized_pos_y, frac_pos_x, frac_pos_y, subpixel_bin_x, subpixel_bin_y,}
    }

    fn key(&self) -> GlyphKey {
        GlyphKey {
            font_id: self.font_key,
            glyph_id: self.glyph.id as u16,
            font_size_bits: self.font_size.to_bits(),
            x_bin: self.subpixel_bin_x,
            y_bin: self.subpixel_bin_y,
        }
    }

    fn frac_offset(&self) -> Vector {
        Vector::new(self.frac_pos_x, self.frac_pos_y)
    }
}



trait UselessTrait2 {
    fn size(&self) -> Size2D<i32, UnknownUnit>;
}
impl UselessTrait2 for Placement {
    fn size(&self) -> Size2D<i32, UnknownUnit> {
        size2(self.width as i32, self.height as i32)
    }
}
