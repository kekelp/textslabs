use crate::*;

impl ContextlessTextRenderer {
    pub(crate) fn new(device: &Device, _queue: &Queue) -> Self {
        let bg_color = Rgba([255, 0, 255, 255]);

        // todo
        // unused memory is wasted memory...?
        // let atlas_size = Limits::downlevel_webgl2_defaults().max_texture_dimension_2d;
        let atlas_size = 256;

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

        let mask_atlas = Atlas::<GrayImage> {
            glyph_cache: LruCache::unbounded_with_hasher(BuildHasherDefault::<FxHasher>::default()),
            pages: vec![AtlasPage::<GrayImage> {
                image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
                last_frame_evicted: 0,
                packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
                texture: Some(mask_texture),
                texture_view: Some(mask_texture_view),
            }],
        };

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

        let color_atlas = Atlas::<RgbaImage> {
            glyph_cache: LruCache::unbounded_with_hasher(BuildHasherDefault::<FxHasher>::default()),
            pages: vec![AtlasPage::<RgbaImage> {
                last_frame_evicted: 0,
                image: RgbaImage::from_pixel(atlas_size, atlas_size, bg_color),
                texture: Some(color_texture),
                texture_view: Some(color_texture_view),
                packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
            }]
        };

        let vertex_buffer_size = 4096 * 9;
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vertices"),
            size: vertex_buffer_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

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
            ],
        };

        let params = Params {
            screen_resolution: Resolution {
                width: 0.0,
                height: 0.0,
            },
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

        let uniforms_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        view_dimension: TextureViewDimension::D2,
                        sample_type: TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        view_dimension: TextureViewDimension::D2,
                        sample_type: TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("atlas bind group layout"),
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: &atlas_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&color_atlas.pages[0].texture_view.as_ref().unwrap()),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&mask_atlas.pages[0].texture_view.as_ref().unwrap()),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
            label: Some("atlas bind group"),
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&atlas_layout, &uniforms_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("pipeline"),
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
                    format: TextureFormat::Bgra8Unorm,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::default(),
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            frame: 1,
            atlas_size,
            tmp_image: Image::new(),
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            color_atlas,
            mask_atlas,
            vertex_buffer,
            vertex_buffer_size,
            pipeline,
            bind_group,

            params,
            params_buffer,
            params_bind_group,
            quads: Vec::<Quad>::with_capacity(300),
        }
    }
}