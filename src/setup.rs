use crate::*;

pub(crate) const INITIAL_BUFFER_SIZE: u64 = 4096;


const ATLAS_BIND_GROUP_LAYOUT: BindGroupLayoutDescriptor = wgpu::BindGroupLayoutDescriptor {
    entries: &[
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
            ty: BindingType::Texture {
                multisampled: false,
                view_dimension: TextureViewDimension::D2,
                sample_type: TextureSampleType::Float { filterable: true },
            },
            count: None,
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
            count: None,
        },
    ],
    label: Some("atlas bind group layout"),
};

/// Configuration parameters for the text renderer.
pub struct TextRendererParams {
    /// Size of texture atlas pages used for glyph caching.
    pub atlas_page_size: AtlasPageSize,
}
impl Default for TextRendererParams {
    fn default() -> Self {
        // 2048 is guaranteed to work everywhere that webgpu supports, and it seems both small enough that it's fine to allocate it upfront even if a smaller one would have been fine, and big enough that even on gpus that could hold 8k textures, I don't feel too bad about using multiple 2k pages instead of a single big 8k one
        // Ideally you'd still with small pages and grow them until the max texture dim, but having cache eviction, multiple pages, AND page growing seems a bit too much for now
        let atlas_page_size = AtlasPageSize::DownlevelWrbgl2Max; // 2048
        Self { atlas_page_size }
    }
}
/// Determines the size of texture atlas pages for glyph storage.
pub enum AtlasPageSize {
    /// Fixed size in pixels.
    Flat(u32),
    /// Use the current device's maximum texture size.
    CurrentDeviceMax,
    /// Use WebGL2 downlevel maximum (2048px).
    DownlevelWrbgl2Max,
    /// Use general downlevel maximum (2048px).
    DownlevelMax,
    /// Use WGPU's default maximum (8192px).
    WgpuMax,
}
impl AtlasPageSize {
    fn size(self, device: &Device) -> u32 {
        match self {
            AtlasPageSize::Flat(i) => i,
            AtlasPageSize::DownlevelWrbgl2Max => Limits::downlevel_defaults().max_texture_dimension_2d,
            AtlasPageSize::DownlevelMax => Limits::downlevel_webgl2_defaults().max_texture_dimension_2d,
            AtlasPageSize::WgpuMax => Limits::default().max_texture_dimension_2d,
            AtlasPageSize::CurrentDeviceMax => device.limits().max_texture_dimension_2d,
        }
    }
}

fn create_vertex_buffer(device: &Device, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("shared vertex buffer"),
        size,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl ContextlessTextRenderer {
    pub fn new_with_params(
        device: &Device,
        _queue: &Queue,
        format: TextureFormat,
        depth_stencil: Option<DepthStencilState>,
        params: TextRendererParams,
    ) -> Self {
        let _srgb = format.is_srgb();
        // todo put this in the uniform and use it
        
        let atlas_size = params.atlas_page_size.size(device);

        let mask_texture = device.create_texture(&TextureDescriptor {
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
        let mask_texture_view = mask_texture.create_view(&TextureViewDescriptor::default());


        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("sampler"),
            min_filter: FilterMode::Nearest,
            mag_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            lod_min_clamp: 0f32,
            lod_max_clamp: 0f32,
            ..Default::default()
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("shader"),
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Sint32x2,
                1 => Uint32,
                2 => Uint32,
                3 => Uint32,
                4 => Float32,
                5 => Uint32,
                6 => Sint16x4,
            ],
        };

        let params = Params {
            screen_resolution_width: 0.0,
            screen_resolution_height: 0.0,
            _pad: [0, 0],
        };

        let params_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("params"),
            size: mem::size_of::<Params>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(mem::size_of::<Params>() as u64),
                },
                count: None,
            }],
            label: Some("uniforms bind group layout"),
        });

        let params_bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: &params_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
            label: Some("uniforms bind group"),
        });

        let atlas_bind_group_layout = device.create_bind_group_layout(&ATLAS_BIND_GROUP_LAYOUT);
        let mask_bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: &atlas_bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&mask_texture_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
            label: Some("atlas bind group"),
        });

        let glyph_cache = LruCache::unbounded_with_hasher(BuildHasherDefault::<FxHasher>::default());

        let mask_atlas_pages = vec![AtlasPage::<GrayImage> {
            image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
            packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
            quads: Vec::<Quad>::with_capacity(300),
            gpu: Some(GpuAtlasPage {
                texture: mask_texture,
                bind_group: mask_bind_group,
            }),
            quad_count_before_render: 0,
        }];

        let color_texture = device.create_texture(&TextureDescriptor {
            label: Some("atlas"),
            size: Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let color_texture_view = color_texture.create_view(&TextureViewDescriptor::default());


        let color_bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: &atlas_bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&color_texture_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
            label: Some("atlas bind group"),
        });

        let color_atlas_pages = vec![AtlasPage::<RgbaImage> {
            image: RgbaImage::from_pixel(atlas_size, atlas_size, Rgba([0, 0, 0, 0])),
            packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
            quads: Vec::<Quad>::with_capacity(300),
            gpu: Some(GpuAtlasPage {
                texture: color_texture,
                bind_group: color_bind_group,
            }),
            quad_count_before_render: 0,
        }];

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&atlas_bind_group_layout, &params_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("textslabs pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::default(),
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let tmp_image = Image::new();
        let frame = 1;
        
        let vertex_buffer = create_vertex_buffer(device, INITIAL_BUFFER_SIZE);
        
        Self {
            frame,
            atlas_size,
            tmp_image,
            mask_atlas_pages,
            color_atlas_pages,
            decorations: Vec::with_capacity(50),
            pipeline,
            atlas_bind_group_layout,
            sampler,
            params,
            params_buffer,
            params_bind_group,
            glyph_cache,
            last_frame_evicted: 0,
            // cached_scaler: None,
            vertex_buffer,
            needs_gpu_sync: true,
        }
    }
}

impl ContextlessTextRenderer {
    pub fn load_to_gpu(&mut self, device: &Device, queue: &Queue) {
        if !self.needs_gpu_sync {
            return;
        }

        let bytes: &[u8] = bytemuck::cast_slice(std::slice::from_ref(&self.params));
        queue.write_buffer(&self.params_buffer, 0, bytes);

        // Calculate total number of quads across all pages plus decorations
        let total_quads = self.mask_atlas_pages.iter().map(|p| p.quads.len()).sum::<usize>()
                        + self.color_atlas_pages.iter().map(|p| p.quads.len()).sum::<usize>()
                        + self.decorations.len();
        
        let required_size = (total_quads * std::mem::size_of::<Quad>()) as u64;
        
        // Grow shared vertex buffer if needed
        if self.vertex_buffer.size() < required_size {
            let min_size = u64::max(required_size, INITIAL_BUFFER_SIZE);
            let growth_size = min_size * 3 / 2;
            let current_growth = self.vertex_buffer.size() * 3 / 2;
            let new_size = u64::max(growth_size, current_growth);
            
            self.vertex_buffer = create_vertex_buffer(device, new_size);
        }

        let mut buffer_offset = 0u64;
        
        for page in &self.mask_atlas_pages {
            if !page.quads.is_empty() {
                let bytes: &[u8] = bytemuck::cast_slice(&page.quads);
                queue.write_buffer(&self.vertex_buffer, buffer_offset, bytes);
                buffer_offset += bytes.len() as u64;
            }
        }
        
        for page in &self.color_atlas_pages {
            if !page.quads.is_empty() {
                let bytes: &[u8] = bytemuck::cast_slice(&page.quads);
                queue.write_buffer(&self.vertex_buffer, buffer_offset, bytes);
                buffer_offset += bytes.len() as u64;
            }
        }
        
        if !self.decorations.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&self.decorations);
            queue.write_buffer(&self.vertex_buffer, buffer_offset, bytes);
        }

        // Handle mask atlas pages
        for page in &mut self.mask_atlas_pages {
            if page.gpu.is_none() {
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("atlas"),
                    size: Extent3d {
                        width: self.atlas_size,
                        height: self.atlas_size,
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

                let bind_group = device.create_bind_group(&BindGroupDescriptor {
                    layout: &self.atlas_bind_group_layout,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::TextureView(&texture_view),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::Sampler(&self.sampler),
                        },
                    ],
                    label: Some("atlas bind group"),
                });
        
                page.gpu = Some(GpuAtlasPage {
                    texture,
                    bind_group,
                })
            }

            queue.write_texture(
                ImageCopyTexture {
                    texture: &page.gpu.as_ref().unwrap().texture,
                    mip_level: 0,
                    origin: Origin3d { x: 0, y: 0, z: 0 },
                    aspect: TextureAspect::All,
                },
                &page.image.as_raw(),
                ImageDataLayout {
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

        // Handle color atlas pages
        for page in &mut self.color_atlas_pages {
            if page.gpu.is_none() {
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("atlas"),
                    size: Extent3d {
                        width: self.atlas_size,
                        height: self.atlas_size,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8Unorm,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let texture_view = texture.create_view(&TextureViewDescriptor::default());

                let bind_group = device.create_bind_group(&BindGroupDescriptor {
                    layout: &self.atlas_bind_group_layout,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::TextureView(&texture_view),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::Sampler(&self.sampler),
                        },
                    ],
                    label: Some("atlas bind group"),
                });
        
                page.gpu = Some(GpuAtlasPage {
                    texture,
                    bind_group,
                })
            }
    
            queue.write_texture(
                ImageCopyTexture {
                    texture: &page.gpu.as_ref().unwrap().texture,
                    mip_level: 0,
                    origin: Origin3d { x: 0, y: 0, z: 0 },
                    aspect: TextureAspect::All,
                },
                &page.image.as_raw(),
                ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(page.image.width() * 4),
                    rows_per_image: None,
                },
                Extent3d {
                    width: page.image.width(),
                    height: page.image.height(),
                    depth_or_array_layers: 1,
                },
            );
        }

        self.needs_gpu_sync = false;
    }
}