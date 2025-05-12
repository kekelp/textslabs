use parley::Rect;

use crate::*;

pub struct TextRenderer {
    pub text_renderer: ContextlessTextRenderer,
    pub scale_cx: ScaleContext,
}

pub struct ContextlessTextRenderer {
    pub frame: u64,
    pub tmp_image: Image,
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<ColorBrush>,

    pub(crate) glyph_cache: LruCache<GlyphKey, Option<StoredGlyph>, BuildHasherDefault<FxHasher>>,
    pub(crate) mask_atlas_pages: Vec<AtlasPage<GrayImage>>,
    pub(crate) last_frame_evicted: u64,
    
    pub(crate) color_atlas_pages: Vec<AtlasPage<RgbaImage>>,
    
    pub atlas_bind_group_layout: BindGroupLayout,
    
    pub params: Params,
    pub sampler: Sampler,
    pub params_buffer: Buffer,
    pub params_bind_group: BindGroup,

    pub pipeline: RenderPipeline,
    pub atlas_size: u32,
}

pub(crate) struct AtlasPage<ImageType> {
    pub quads: Vec<Quad>,
    pub(crate) packer: BucketedAtlasAllocator,
    pub(crate) image: ImageType,
    pub vertex_buffer_size: u64,
    pub gpu: Option<GpuAtlasPage>,
}

pub(crate) struct GpuAtlasPage {
    pub vertex_buffer: Buffer,
    pub texture: Texture, // the format here has to match the image type...
    pub bind_group: BindGroup,
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

    fn prepare_selection_rect(&mut self, rect: parley::Rect, left: f32, top: f32) {
        
        let left = left as i32;
        let top = top as i32;

        let x0 = left + rect.x0 as i32;
        let x1 = left + rect.x1 as i32;
        let y0 = top + rect.y0 as i32;
        let y1 = top + rect.y1 as i32;


        let quad = Quad {
            pos: [x0, y0],
            dim: [(x1 - x0) as u16, (y1 - y0) as u16],
            // dim: [(rect.x1 - rect.x0) as u16, (rect.y1 - rect.y0) as u16],
            color: 0x00_00_55_ff,
            uv_origin: [0, 0],
            depth: 0.0,
            flags: 2, // todo make names for these
        };
        self.mask_atlas_pages[0].quads.push(quad);
    }
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
    pub x_bin: SubpixelBin::<4>,
    /// Binning of fractional Y offset
    pub y_bin: SubpixelBin::<4>,
    // /// [`CacheKeyFlags`]
    // pub flags: CacheKeyFlags,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SubpixelBin<const N: u8>(pub u8);

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

impl<const N: u8> SubpixelBin<N> {    
    pub fn as_float(&self) -> f32 {
        if self.0 == 0 {
            0.0
        } else {
            (2 * self.0 - 1) as f32 / (2 * N) as f32
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Zeroable, Pod)]
pub struct Quad {
    pub pos: [i32; 2],
    pub dim: [u16; 2],
    pub uv_origin: [u16; 2],
    pub color: u32,
    pub depth: f32,
    pub flags: u32,
}

fn make_quad(glyph: &GlyphWithContext, stored_glyph: &StoredGlyph) -> Quad {
    let y = glyph.quantized_pos_y - stored_glyph.placement_top as i32;
    let x = glyph.quantized_pos_x + stored_glyph.placement_left as i32;

    let (dim_x, dim_y) = (stored_glyph.alloc.rectangle.min.x, stored_glyph.alloc.rectangle.min.y);
    let (size_x, size_y) = (stored_glyph.size.width, stored_glyph.size.height);

    let (color, flags) = match stored_glyph.content_type {
        Content::Mask => (glyph.color, 0),
        Content::Color => (0xff_ff_ff_ff, 1), // todo funny blending?
        Content::SubpixelMask => unreachable!(),
    };
    return Quad {
        pos: [x, y],
        dim: [size_x as u16, size_y as u16],
        uv_origin: [dim_x as u16, dim_y as u16],
        color,
        flags,
        depth: 0.0,
    };
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorBrush(pub [u8; 4]);
impl Default for ColorBrush {
    fn default() -> Self {
        Self([0, 0, 0, 255])
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// The width of the screen in pixels.
    pub screen_resolution_width: f32,
    /// The height of the screen in pixels.
    pub screen_resolution_height: f32,
    pub _pad: [u32; 2],
}

impl TextRenderer {
    pub fn new_with_params(device: &Device, _queue: &Queue, params: TextRendererParams) -> Self {
        Self {
            scale_cx: ScaleContext::new(),
            text_renderer: ContextlessTextRenderer::new_with_params(device, _queue, params),
        }
    }

    pub fn new(device: &Device, queue: &Queue) -> Self {
        Self::new_with_params(device, queue, TextRendererParams::default())
    }

    pub fn update_resolution(&mut self, width: f32, height: f32) {
        self.text_renderer.update_resolution(width, height);
    }

    pub fn clear(&mut self) {
        self.text_renderer.clear();
    }

    pub fn prepare_layout(&mut self, layout: &Layout<ColorBrush>, left: f32, top: f32) {
        self.text_renderer.prepare_layout(layout, &mut self.scale_cx, left, top);
    }

    pub fn prepare_text_box(&mut self, text_box: &mut TextBox) {
        let (left, top) = text_box.pos();
        let (left, top) = (left as f32, top as f32);
        self.text_renderer.prepare_layout(text_box.layout(), &mut self.scale_cx, left, top);

        text_box.selection().geometry_with(&text_box.layout, |rect, _line_i| {
            self.text_renderer.prepare_selection_rect(rect, left, top);
        });
    }

    pub fn gpu_load(&mut self, device: &Device, queue: &Queue) {
        self.text_renderer.gpu_load(device, queue);
    }

    pub fn render(&self, pass: &mut RenderPass<'_>) {
        self.text_renderer.render(pass);
    }

    pub fn gpu_load_atlas_debug(&mut self, _device: &Device, queue: &Queue) {
        let atlas_size = self.text_renderer.atlas_size;
        
        for (i, page) in self.text_renderer.mask_atlas_pages.iter_mut().enumerate() {
            let x_offset = i as i32 * (atlas_size as i32 + 10);

            page.quads = vec![Quad {
                pos: [x_offset, 0],
                dim: [atlas_size as u16, atlas_size as u16],
                uv_origin: [0, 0],
                color: 0xff0000ff,
                depth: 0.0,
                flags: 0,
            }];
            
            let bytes: &[u8] = bytemuck::cast_slice(&page.quads);
            queue.write_buffer(&page.gpu.as_ref().unwrap().vertex_buffer, 0, &bytes);
        }
    
        for (i, page) in self.text_renderer.color_atlas_pages.iter_mut().enumerate() {
            let x_offset = i as i32 * (atlas_size as i32 + 10);
            
            page.quads = vec![Quad {
                pos: [x_offset, atlas_size as i32 + 10],
                dim: [atlas_size as u16, atlas_size as u16],
                uv_origin: [0, 0],
                color: 0xffffffff,
                depth: 0.0,
                flags: 1,
            }];
            
            let bytes: &[u8] = bytemuck::cast_slice(&page.quads);
            queue.write_buffer(&page.gpu.as_ref().unwrap().vertex_buffer, 0, &bytes);
        }
    }
    
    pub fn render_atlas_debug(&self, pass: &mut RenderPass<'_>) {
        if self.text_renderer.mask_atlas_pages[0].quads.is_empty() { return }
        
        pass.set_pipeline(&self.text_renderer.pipeline);
        pass.set_bind_group(1, &self.text_renderer.params_bind_group, &[]);
        
        for page in &self.text_renderer.mask_atlas_pages {
            pass.set_bind_group(0, &page.gpu.as_ref().unwrap().bind_group, &[]);
            pass.set_vertex_buffer(0, page.gpu.as_ref().unwrap().vertex_buffer.slice(..));
            pass.draw(0..4, 0..1);
        }

        for page in &self.text_renderer.color_atlas_pages {
            pass.set_bind_group(0, &page.gpu.as_ref().unwrap().bind_group, &[]);
            pass.set_vertex_buffer(0, page.gpu.as_ref().unwrap().vertex_buffer.slice(..));
            pass.draw(0..4, 0..1);
        }
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
        pass.set_bind_group(1, &self.params_bind_group, &[]);

        for page in &self.mask_atlas_pages {
            if !page.quads.is_empty() {
                pass.set_bind_group(0, &page.gpu.as_ref().unwrap().bind_group, &[]);
                pass.set_vertex_buffer(0, page.gpu.as_ref().unwrap().vertex_buffer.slice(..));
                pass.draw(0..4, 0..page.quads.len() as u32);
            }
        }

        for page in &self.color_atlas_pages {
            if !page.quads.is_empty() {
                pass.set_bind_group(0, &page.gpu.as_ref().unwrap().bind_group, &[]);
                pass.set_vertex_buffer(0, page.gpu.as_ref().unwrap().vertex_buffer.slice(..));
                pass.draw(0..4, 0..page.quads.len() as u32);
            }
        }
    }

    pub fn update_resolution(&mut self, width: f32, height: f32) {
        self.params.screen_resolution_width = width;
        self.params.screen_resolution_height = height;
    }

    pub fn clear(&mut self) {
        self.frame += 1;

        for page in &mut self.mask_atlas_pages {
            page.quads.clear();
        }
        for page in &mut self.color_atlas_pages {
            page.quads.clear();
        }
    }

    fn prepare_layout(&mut self, layout: &Layout<ColorBrush>, scale_cx: &mut ScaleContext, left: f32, top: f32) {
        for line in layout.lines() {
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        self.prepare_glyph_run(&glyph_run, scale_cx, left, top);
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
        top: f32
    ) {
        let mut run_x = left + glyph_run.offset();
        let run_y = top + glyph_run.baseline();
        let style = glyph_run.style();

        let run = glyph_run.run();

        let font = run.font();
        let font_size = run.font_size();
        let font_ref = FontRef::from_index(font.data.as_ref(), font.index as usize).unwrap();
        let font_key = font.data.id();

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
                    let quad = make_quad(&glyph_ctx, stored_glyph);
                    let page = stored_glyph.page as usize;

                    match stored_glyph.content_type {
                        Content::Mask => self.mask_atlas_pages[page].quads.push(quad),
                        Content::Color => self.color_atlas_pages[page].quads.push(quad),
                        Content::SubpixelMask => unreachable!()
                    };
                }
            } else {
                if let Some((quad, stored_glyph)) = self.prepare_glyph(&glyph_ctx, &mut scaler) {
                    // todo: make more symmetric
                    let page = stored_glyph.page as usize;

                    match stored_glyph.content_type {
                        Content::Mask => self.mask_atlas_pages[page].quads.push(quad),
                        Content::Color => self.color_atlas_pages[page].quads.push(quad),
                        Content::SubpixelMask => unreachable!()
                    };
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
    fn render_glyph(&mut self, glyph: &GlyphWithContext, scaler: &mut Scaler) -> (Content, Placement) {
        self.tmp_image.clear();
        Render::new(SOURCES)
            .format(Format::Alpha)
            .offset(glyph.frac_offset())
            .render_into(scaler, glyph.glyph.id, &mut self.tmp_image);
        return (self.tmp_image.content, self.tmp_image.placement);
    }

    /// Rasterizes the glyph in a texture atlas and returns a Quad that can be used to render it, or None if the glyph was just empty (like a space).
    fn prepare_glyph(&mut self, glyph: &GlyphWithContext, scaler: &mut Scaler) -> Option<(Quad, StoredGlyph)> {
        let (content, placement) = self.render_glyph(&glyph, scaler);
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
                return self.store_glyph(glyph, size, &alloc, page, &placement, content);
            }
            
            // Try evicting glyphs from previous frames and retry
            if self.needs_evicting(self.frame) {
                self.evict_old_glyphs();
                
                if let Some(alloc) = self.pack_rectangle(size, content, page) {
                    return self.store_glyph(glyph, size, &alloc, page, &placement, content);
                }
            }
        }
        
        // Create a new page and try to allocate there
        let new_page: usize = self.make_new_page(content);
        if let Some(alloc) = self.pack_rectangle(size, content, new_page) {
            return self.store_glyph(glyph, size, &alloc, new_page, &placement, content);
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
        ) -> Option<(Quad, StoredGlyph)> {
        self.copy_glyph_to_atlas(size, alloc, page, content_type);
        let stored_glyph = StoredGlyph::create(alloc, placement, page, self.frame, content_type);
        self.glyph_cache.push(glyph.key(), Some(stored_glyph));
        let quad = make_quad(glyph, &stored_glyph);
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
                // todo, deduplicate these with the ones in Setup
                self.mask_atlas_pages.push(AtlasPage::<GrayImage> {
                    image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
                    packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
                    vertex_buffer_size: 4096 * 9,
                    quads: Vec::<Quad>::with_capacity(300),
                    gpu: None, // will be created later
                });
                return self.mask_atlas_pages.len() - 1;
            },
            Content::Color => {
                self.color_atlas_pages.push(AtlasPage::<RgbaImage> {
                    image: RgbaImage::from_pixel(atlas_size, atlas_size, Rgba([0, 0, 0, 0])),
                    packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
                    vertex_buffer_size: 4096 * 9,
                    quads: Vec::<Quad>::with_capacity(300),
                    gpu: None, // will be created later
                });
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
        let glyph_x = run_x + glyph.x;
        let glyph_y = run_y - glyph.y;

        let (quantized_pos_x, frac_pos_x, subpixel_bin_x) = quantize(glyph_x);
        let (quantized_pos_y, frac_pos_y, subpixel_bin_y) = quantize(glyph_y);

        let color = ((color.0[0] as u32) << 0)
            + ((color.0[1] as u32) << 8)
            + ((color.0[2] as u32) << 16)
            + ((color.0[3] as u32) << 24);

        Self { glyph, color, font_key, font_size, quantized_pos_x, quantized_pos_y, frac_pos_x, frac_pos_y, subpixel_bin_x, subpixel_bin_y,}
    }

    fn key(&self) -> GlyphKey {
        GlyphKey {
            font_id: self.font_key,
            glyph_id: self.glyph.id,
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
