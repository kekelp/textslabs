use swash::zeno::Placement;

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

    pub(crate) color_atlas: Atlas<RgbaImage>,
    pub(crate) mask_atlas: Atlas<GrayImage>,
    
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
    pub(crate) glyph_cache: LruCache<GlyphKey, Option<StoredGlyph>, BuildHasherDefault<FxHasher>>,
    pub(crate) pages: Vec<AtlasPage<ImageType>>,
}

pub(crate) struct AtlasPage<ImageType> {
    pub(crate) packer: BucketedAtlasAllocator,
    pub(crate) last_frame_evicted: u64,
    pub(crate) image: ImageType,
    pub(crate) texture: Option<Texture>, // the format here has to match the image type...
    pub(crate) texture_view: Option<TextureView>,
}


impl Atlas<GrayImage> {
    pub fn gpu_load(&mut self, queue: &Queue, device: &Device, atlas_size: u32) {
        for page in &mut self.pages {
            if page.texture.is_none() {
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("atlas"),
                    size: Extent3d {
                        width: atlas_size,
                        height: atlas_size,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::R8Unorm,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let texture_view = texture.create_view(&TextureViewDescriptor::default());
                page.texture = Some(texture);       
                page.texture_view = Some(texture_view);       
            }

            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &page.texture.as_ref().unwrap(),
                    mip_level: 0,
                    origin: Origin3d { x: 0, y: 0, z: 0 },
                    aspect: TextureAspect::All,
                },
                &page.image.as_raw(),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(page.image.width()),
                    rows_per_image: None,
                },
                Extent3d {
                    width: page.image.width(),
                    height: page.image.height(),
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

impl<ImageType> AtlasPage<ImageType> {
    fn evict_old_glyphs(&mut self) {
        todo!()
    }

    fn needs_evicting(&self, current_frame: u64) -> bool {
        self.last_frame_evicted == current_frame
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
}

fn make_quad(glyph: &GlyphWithContext, stored_glyph: &StoredGlyph) -> Quad {
    let scale_factor = 1.0; // todo, what is this
    let line_y = (glyph.run_y * scale_factor).round() as i32;
    let y = line_y + glyph.quantized_pos_y - stored_glyph.placement_top as i32;
    let x = glyph.quantized_pos_x + stored_glyph.placement_left as i32;

    let (dim_x, dim_y) = (stored_glyph.alloc.rectangle.min.x, stored_glyph.alloc.rectangle.min.y);
    let (size_x, size_y) = (stored_glyph.alloc.rectangle.width(), stored_glyph.alloc.rectangle.height());
    return Quad {
        pos: [x, y],
        dim: [size_x as u16, size_y as u16],
        uv_origin: [dim_x as u16, dim_y as u16],
        color: glyph.color,
        depth: 0.0,
    };
}

/// A glyph as stored in a glyph atlas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StoredGlyph {
    page: u16,
    frame: u64,
    alloc: Allocation,
    placement_left: i32,
    placement_top: i32,
}
impl StoredGlyph {
    fn create(alloc: &Allocation, placement: &Placement, page: usize, frame: u64) -> StoredGlyph {
        StoredGlyph {
            page: page as u16,
            frame: frame,
            alloc: alloc.clone(),
            placement_left: placement.left,
            placement_top: placement.top
        }
    }
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

impl TextRenderer {
    pub fn new(device: &Device, _queue: &Queue) -> Self {
        Self {
            scale_cx: ScaleContext::new(),
            text_renderer: ContextlessTextRenderer::new(device, _queue),
        }
    }

    pub fn clear(&mut self) {
        self.text_renderer.clear();
    }

    pub fn prepare_layout(&mut self, layout: &Layout<ColorBrush>) {
        self.text_renderer.prepare_layout(layout, &mut self.scale_cx);
    }

    pub fn gpu_load(&mut self, device: &Device, queue: &Queue) {
        self.text_renderer.gpu_load(device, queue);
    }

    pub fn render(&self, pass: &mut RenderPass<'_>) {
        self.text_renderer.render(pass);
    }

}


const SOURCES: &[Source; 3] = &[
    Source::ColorOutline(0),
    Source::ColorBitmap(StrikeWith::BestFit),
    Source::Outline,
];

impl ContextlessTextRenderer {
    pub fn render(&self, pass: &mut RenderPass<'_>) {
        if self.quads.is_empty() { return }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &self.params_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..4, 0..self.quads.len() as u32);
    }

    pub fn clear(&mut self) {
        self.frame += 1;
        self.quads.clear();
    }

    fn prepare_layout(&mut self, layout: &Layout<ColorBrush>, scale_cx: &mut ScaleContext) {
        for line in layout.lines() {
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        self.prepare_glyph_run(&glyph_run, scale_cx);
                    }
                    PositionedLayoutItem::InlineBox(_inline_box) => {}
                }
            }
        }
    }

    fn gpu_load(&mut self, device: &Device, queue: &Queue) {
        // todo: check what is actually needed
        queue.write_buffer(&self.params_buffer, 0, unsafe {
            core::slice::from_raw_parts(
                &self.params as *const Params as *const u8,
                mem::size_of::<Params>(),
            )
        });

        let bytes: &[u8] = bytemuck::cast_slice(&self.quads);
        queue.write_buffer(&self.vertex_buffer, 0, &bytes);
        
        self.mask_atlas.gpu_load(queue, device, self.atlas_size);
        // todo: color atlas
    }

    fn prepare_glyph_run(
        &mut self,
        glyph_run: &GlyphRun<'_, ColorBrush>,
        scale_cx: &mut ScaleContext,
    ) {
        let mut run_x = glyph_run.offset();
        let run_y = glyph_run.baseline();
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
            let glyph_ctx = GlyphWithContext::new(glyph, run_x, run_y, font_key, font_size, style.brush.color);

            if let Some(stored_glyph) = self.mask_atlas.glyph_cache.get(&glyph_ctx.key()) {
                if let Some(stored_glyph) = stored_glyph {
                    let quad = make_quad(&glyph_ctx, stored_glyph);
                    self.quads.push(quad);
                }
            } else {
                if let Some(quad) = self.store_glyph(&glyph_ctx, &mut scaler) {
                    self.quads.push(quad);
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

    // fn push_quad(&mut self) {
    //     let quad = Quad {
    //         pos: [x, y],
    //         dim: [size.width as u16, size.height as u16],
    //         uv: [alloc.rectangle.min.x as u16, alloc.rectangle.min.y as u16],
    //         color,
    //         depth: 0.0,
    //     };
    //     self.quads.push(quad);
    // }

    fn copy_glyph_to_atlas(&mut self, size: Size2D<i32, UnknownUnit>, alloc: &Allocation) {
        for y in 0..size.height as i32 {
            let src_start = (y as usize) * (size.width as usize);
            let src_slice =
                &self.tmp_image.data[src_start..(src_start + size.width as usize)];

            let dst_y = (alloc.rectangle.min.y + y) as u32;
            let dst_x = alloc.rectangle.min.x as u32;

            // todo dont do this
            let layout = self.mask_atlas.pages[0].image.as_flat_samples().layout;
            let mut samples = self.mask_atlas.pages[0].image.as_flat_samples_mut();
            let samples = samples.as_mut_slice();

            let dst_start =
                (dst_y as usize) * layout.height_stride + (dst_x as usize) * layout.width_stride;

            samples[dst_start..(dst_start + size.width as usize)].copy_from_slice(src_slice);
        }
    }

    /// Render a glyph into the `self.tmp_swash_image` buffer
    // this is going to have to return the Content (color/mask) as well
    fn render_glyph(&mut self, glyph: &GlyphWithContext, scaler: &mut Scaler) -> Placement {
        self.tmp_image.clear();
        Render::new(SOURCES)
            .format(Format::Alpha)
            .offset(glyph.frac_offset())
            .render_into(scaler, glyph.glyph.id, &mut self.tmp_image);
        return self.tmp_image.placement;
    }

    /// Rasterizes the glyph in a texture atlas and returns a Quad that can be used to render it, or None if the glyph was just empty (like a space).
    fn store_glyph(&mut self, glyph: &GlyphWithContext, scaler: &mut Scaler) -> Option<Quad> {
        let glyph_key = glyph.key();
        let placement = self.render_glyph(&glyph, scaler);

        let size = placement.size();
        if placement.size().is_empty() {
            self.mask_atlas.glyph_cache.push(glyph_key, None);
            return None;
        }

        let mut page = 0;

        loop {
            if page >= self.mask_atlas.pages.len() {
                self.make_new_mask_atlas_page();
            }

            if let Some(alloc) = self.mask_atlas.pages[0].packer.allocate(size) {
                self.copy_glyph_to_atlas(size, &alloc);
    
                let stored_glyph = StoredGlyph::create(&alloc, &placement, page, self.frame);
                self.mask_atlas.glyph_cache.push(glyph_key, Some(stored_glyph));
    
                let quad = make_quad(glyph, &stored_glyph);
    
                return Some(quad);
            } else {
                if self.mask_atlas.pages[page].needs_evicting(self.frame) {
                    self.mask_atlas.pages[page].evict_old_glyphs();
                    // continue loop and retry on the same page
                } else {
                    // retry on the next page
                    page += 1;
                }
            };
        }

    }

    fn make_new_mask_atlas_page(&mut self) {
        let atlas_size = self.atlas_size;

        self.mask_atlas.pages.push(AtlasPage::<GrayImage> {
            image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
            last_frame_evicted: 0,
            packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
            texture: None,
            texture_view: None,
        });

    }
}

/// A glyph with the context in which it is being drawn 
struct GlyphWithContext {
    glyph: Glyph,
    color: u32,
    run_y: f32,
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
    fn new(glyph: Glyph, run_x: f32, run_y: f32, font_key: u64, font_size: f32, color: Rgba<u8>) -> Self {
        let glyph_x = run_x + glyph.x;
        let glyph_y = run_y - glyph.y;

        let (quantized_pos_x, frac_pos_x, subpixel_bin_x) = quantize(glyph_x);
        let (quantized_pos_y, frac_pos_y, subpixel_bin_y) = quantize(glyph_y);

        let color = ((color[0] as u32) << 0)
            + ((color[1] as u32) << 8)
            + ((color[2] as u32) << 16)
            + ((color[3] as u32) << 24);

        Self {
            glyph,
            color,
            run_y,
            font_key,
            font_size,
            quantized_pos_x,
            quantized_pos_y,
            frac_pos_x,
            frac_pos_y,
            subpixel_bin_x,
            subpixel_bin_y,
        }
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
