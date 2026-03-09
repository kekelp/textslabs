use crate::*;

pub(crate) struct TextRenderer {
    // wgpu handles (these are cheap to clone)
    pub(crate) device: Device,
    pub(crate) queue: Queue,

    // Combined texture arrays and single bind group
    pub(crate) mask_texture_array: Texture,
    pub(crate) color_texture_array: Texture,
    pub(crate) bind_group_layout: BindGroupLayout,
    pub(crate) bind_group: BindGroup,

    pub(crate) sampler: Sampler,
    pub(crate) params_buffer: Buffer,

    pub(crate) pipeline: RenderPipeline,
    pub(crate) atlas_size: u32,

    pub(crate) vertex_buffer: Buffer,
    pub(crate) box_data_buffer: Buffer,

    pub(crate) srgb: bool,
}

impl TextRenderer {
    /// Returns the source of the composable text shader.
    pub fn composable_shader_source() -> &'static str {
        include_str!("shaders/textslabs.slang")
    }

    /// Get the vertex buffer for external rendering
    pub fn vertex_buffer(&self) -> &Buffer {
        &self.vertex_buffer
    }

    /// Get the bind group for external rendering.
    pub fn bind_group(&self) -> BindGroup {
        self.bind_group.clone()
    }

    /// Get the bind group layout for external rendering.
    pub fn bind_group_layout(&self) -> BindGroupLayout {
        self.bind_group_layout.clone()
    }

    /// Get the render pipeline for external rendering
    pub fn pipeline(&self) -> &RenderPipeline {
        &self.pipeline
    }

    /// Get mask texture array for external rendering
    pub fn mask_texture_array(&self) -> &Texture {
        &self.mask_texture_array
    }

    /// Get color texture array for external rendering
    pub fn color_texture_array(&self) -> &Texture {
        &self.color_texture_array
    }

    /// Get the atlas sampler for external rendering
    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }
}

impl TextRenderer {
    /// Render all prepared text using the provided render pass.
    pub fn render(&self, pass: &mut RenderPass, render_data: &RenderData) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

        // Calculate total instance count
        let total_instances = render_data.glyph_quads.len();

        if total_instances > 0 {
            // Single draw call for all instances
            pass.draw(0..4, 0..(total_instances as u32));
        }
    }

    /// Load the render data to the GPU.
    pub fn load_to_gpu(&mut self, render_data: &mut RenderData) {
        if !render_data.needs_glyph_sync && !render_data.needs_box_data_sync && !render_data.needs_texture_array_rebuild {
            return;
        }

        // Update uniform buffer
        let bytes: &[u8] = bytemuck::cast_slice(std::slice::from_ref(&render_data.params));
        self.queue.write_buffer(&self.params_buffer, 0, bytes);

        // Rebuild texture arrays if needed
        if render_data.needs_texture_array_rebuild {
            self.rebuild_texture_arrays(render_data);
            render_data.needs_texture_array_rebuild = false;
        } else {
            self.update_texture_arrays(render_data);
        }

        // Sync quads buffer if needed
        if render_data.needs_glyph_sync {
            let required_size = (render_data.glyph_quads.len() * std::mem::size_of::<GlyphQuad>()) as u64;

            // Grow shared vertex buffer if needed
            if self.vertex_buffer.size() < required_size {
                let min_size = u64::max(required_size, INITIAL_BUFFER_SIZE);
                let growth_size = min_size * 3 / 2;
                let current_growth = self.vertex_buffer.size() * 3 / 2;
                let new_size = u64::max(growth_size, current_growth);

                self.vertex_buffer = create_vertex_buffer(&self.device, new_size);
                self.recreate_bind_group();
            }

            // Write all quads to vertex buffer
            if !render_data.glyph_quads.is_empty() {
                let bytes: &[u8] = bytemuck::cast_slice(&render_data.glyph_quads);
                self.queue.write_buffer(&self.vertex_buffer, 0, bytes);
            }

            render_data.needs_glyph_sync = false;
        }

        // Sync box_data buffer if needed
        if render_data.needs_box_data_sync {
            let box_data_required_size = (render_data.box_data.len() * std::mem::size_of::<BoxGpu>()) as u64;

            // Grow box_data buffer if needed
            if self.box_data_buffer.size() < box_data_required_size {
                let min_size = u64::max(box_data_required_size, 1024 * std::mem::size_of::<BoxGpu>() as u64);
                let growth_size = min_size * 3 / 2;
                let current_growth = self.box_data_buffer.size() * 3 / 2;
                let new_size = u64::max(growth_size, current_growth);

                self.box_data_buffer = create_box_data_buffer(&self.device, new_size);
                self.recreate_bind_group();
            }

            // Write all box_data to buffer
            if render_data.box_data.len() != 0 {
                let bytes: &[u8] = bytemuck::cast_slice(&render_data.box_data.as_slice());
                self.queue.write_buffer(&self.box_data_buffer, 0, bytes);
            }

            render_data.needs_box_data_sync = false;
        }
    }
}

impl TextRenderer {
    pub(crate) fn rebuild_texture_arrays(&mut self, render_data: &mut RenderData) {
        let (mask_texture_array, color_texture_array) = rebuild_texture_arrays(
            &self.device,
            &self.queue,
            self.atlas_size,
            &mut render_data.mask_atlas_pages,
            &mut render_data.color_atlas_pages,
            self.srgb,
        );

        self.mask_texture_array = mask_texture_array;
        self.color_texture_array = color_texture_array;

        // Rebuild bind group after textures are updated
        self.recreate_bind_group();
    }

    pub(crate) fn update_texture_arrays(&mut self, render_data: &mut RenderData) {
        for (i, page) in render_data.mask_atlas_pages.iter_mut().enumerate() {
            if page.needs_upload {
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.mask_texture_array,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &page.image.as_raw(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(page.image.width()),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: page.image.width(),
                        height: page.image.height(),
                        depth_or_array_layers: 1,
                    },
                );
                page.needs_upload = false;
            }
        }

        // Update only dirty color texture array pages
        for (i, page) in render_data.color_atlas_pages.iter_mut().enumerate() {
            if page.needs_upload {
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.color_texture_array,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &page.image.as_raw(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(page.image.width() * 4),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: page.image.width(),
                        height: page.image.height(),
                        depth_or_array_layers: 1,
                    },
                );
                page.needs_upload = false;
            }
        }
    }


    pub(crate) fn recreate_bind_group(&mut self) {
        let bind_group = create_bind_group(
            &self.device,
            &self.mask_texture_array,
            &self.color_texture_array,
            &self.vertex_buffer,
            &self.sampler,
            &self.params_buffer,
            &self.box_data_buffer,
            &self.bind_group_layout,
        );

        self.bind_group = bind_group;
    }
}
