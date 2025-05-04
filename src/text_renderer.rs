use crate::*;


impl GlyphKey {
    pub fn new(font_id: u64, glyph_id: u16, font_size: f32, pos: (f32, f32)) -> (Self, i32, i32) {
        let (x, x_bin) = SubpixelBin::new(pos.0);
        let (y, y_bin) = SubpixelBin::new(pos.1);
        (
            Self {
                font_id,
                glyph_id,
                font_size_bits: font_size.to_bits(),
                x_bin,
                y_bin,
            },
            x,
            y,
        )
    }

    pub fn frac_offset(&self) -> Vector {
        Vector {
            x: self.x_bin.as_float(),
            y: self.y_bin.as_float(),
        }
    }
}

impl TextRenderer {
    pub fn new(device: &Device, _queue: &Queue) -> Self {
        Self {
            scale_cx: ScaleContext::new(),
            text_renderer: ContextlessTextRenderer::new(device, _queue),
        }
    }

    pub fn prepare(&mut self, layout: &Layout<ColorBrush>) {
        self.text_renderer
            .prepare(layout, &mut self.scale_cx);
    }

    pub fn gpu_load(&mut self, queue: &Queue) {
        self.text_renderer.gpu_load(queue);
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
        // if self.quads.is_empty() { return }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &self.params_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..4, 0..self.quads.len() as u32);
    }

    fn prepare(&mut self, layout: &Layout<ColorBrush>, scale_cx: &mut ScaleContext) {
        // Iterate over laid out lines
        for line in layout.lines() {
            // Iterate over GlyphRun's within each line
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

    fn gpu_load(&mut self, queue: &Queue) {
        // todo: check what is actually needed
        queue.write_buffer(&self.params_buffer, 0, unsafe {
            core::slice::from_raw_parts(
                &self.params as *const Params as *const u8,
                mem::size_of::<Params>(),
            )
        });

        let bytes: &[u8] = bytemuck::cast_slice(&self.quads);
        queue.write_buffer(&self.vertex_buffer, 0, &bytes);
        
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.mask_atlas.texture,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: 0 },
                aspect: TextureAspect::All,
            },
            &self.mask_atlas.image.as_raw(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.mask_atlas.image.width()),
                rows_per_image: None,
            },
            Extent3d {
                width: self.mask_atlas.image.width(),
                height: self.mask_atlas.image.height(),
                depth_or_array_layers: 1,
            },
        );

        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.color_atlas.texture,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: 0 },
                aspect: TextureAspect::All,
            },
            &self.color_atlas.image.as_raw(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.color_atlas.image.width() * 4),
                rows_per_image: None,
            },
            Extent3d {
                width: self.color_atlas.image.width(),
                height: self.color_atlas.image.height(),
                depth_or_array_layers: 1,
            },
        );
    }

    fn prepare_glyph_run(
        &mut self,
        glyph_run: &GlyphRun<'_, ColorBrush>,
        scale_cx: &mut ScaleContext,
    ) {
        // Resolve properties of the GlyphRun
        let mut run_x = glyph_run.offset();
        let run_y = glyph_run.baseline();
        let style = glyph_run.style();
        let color_u8s = style.brush.color;
        let color = ((color_u8s[0] as u32) << 0)
            + ((color_u8s[1] as u32) << 8)
            + ((color_u8s[2] as u32) << 16)
            + ((color_u8s[3] as u32) << 24);

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
            let glyph_x = run_x + glyph.x;
            let glyph_y = run_y - glyph.y;
            run_x += glyph.advance;

            let (cache_key, pos_x, pos_y) =
                GlyphKey::new(font_key, glyph.id, font_size, (glyph_x, glyph_y));

            match self.tmp_image.content {
                Content::Mask => {
                    if let Some(alloc) = self.mask_atlas.glyph_cache.get(&cache_key) {
                        // eprintln!("cache hit {:?}", cache_key);
                    } else {
                        // eprintln!("cache miss {:?}", cache_key);

                        self.render_glyph(glyph, cache_key, &mut scaler);

                        let size = self.tmp_image.size();

                        if size.is_empty() {
                            continue;
                        }

                        if let Some(alloc) = self.mask_atlas.packer.allocate(size) {
                            self.copy_glyph_to_atlas(size, &alloc);
                            self.mask_atlas.glyph_cache.push(cache_key, alloc);

                            let scale_factor = 1.0; // todo, what is this
                            let line_y = (run_y * scale_factor).round() as i32;
                            let y = line_y + pos_y - self.tmp_image.placement.top as i32;
                            let x = pos_x + self.tmp_image.placement.left as i32;
        
                            let quad = Quad {
                                pos: [x, y],
                                dim: [size.width as u16, size.height as u16],
                                uv: [alloc.rectangle.min.x as u16, alloc.rectangle.min.y as u16],
                                color,
                                depth: 0.0,
                            };
                            self.quads.push(quad);
                        } else {
                            todo!("grow the atlas or figure other reasons why the alloc fails")
                        }
                    }
                }
                Content::SubpixelMask => unimplemented!(),
                Content::Color => {
                    // let alloc = self.color_atlas.allocate(size, cache_key);
                    // let glyph_x = alloc.rectangle.min.x;
                    // let glyph_y = alloc.rectangle.min.y;

                    // let row_size = glyph_width as usize * 4;
                    // for (pixel_y, row) in
                    //     self.tmp_swash_image.data.chunks_exact(row_size).enumerate()
                    // {
                    //     // todo: surely this could just be a single memcpy?
                    //     for (pixel_x, pixel) in row.chunks_exact(4).enumerate() {
                    //         let (pixel_x, pixel_y) = (pixel_x as i32, pixel_y as i32);
                    //         let x = glyph_x + pixel_x;
                    //         let y = glyph_y + pixel_y;
                    //         let color = Rgba(pixel.try_into().expect("Not RGBA"));
                    //         *self.color_atlas.image.get_pixel_mut(x as u32, y as u32) = color;
                    //     }
                    // }
                }
            }
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

            let layout = self.mask_atlas.image.as_flat_samples().layout;
            let mut samples = self.mask_atlas.image.as_flat_samples_mut();
            let samples = samples.as_mut_slice();

            let dst_start =
                (dst_y as usize) * layout.height_stride + (dst_x as usize) * layout.width_stride;

            samples[dst_start..(dst_start + size.width as usize)].copy_from_slice(src_slice);
        }
    }

    /// Render a glyph into the `self.tmp_swash_image` buffer
    fn render_glyph(&mut self, glyph: Glyph, cache_key: GlyphKey, scaler: &mut Scaler) {
        self.tmp_image.clear();
        Render::new(SOURCES)
            .format(Format::Alpha)
            .offset(cache_key.frac_offset())
            .render_into(scaler, glyph.id, &mut self.tmp_image);
    }
}



trait UselessTrait2 {
    fn size(&self) -> Size2D<i32, UnknownUnit>;
}
impl UselessTrait2 for Image {
    fn size(&self) -> Size2D<i32, UnknownUnit> {
        size2(self.placement.width as i32, self.placement.height as i32)
    }
}
