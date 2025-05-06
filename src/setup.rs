use crate::*;

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

impl ContextlessTextRenderer {
    pub(crate) fn new(device: &Device, _queue: &Queue) -> Self {
        // 2048 is guaranteed to work everywhere that webgpu supports, and it seems both small enough that it's fine to allocate it upfront even if a smaller one would have been fine, and big enough that even on gpus that could hold 8k textures, I don't feel too bad about using multiple 2k pages instead of a single big 8k one
        // Ideally you'd still with small pages and grow them until the max texture dim, but having both cache eviction, multiple pages, AND page growing seems a bit too much for now
        let atlas_size = Limits::downlevel_webgl2_defaults().max_texture_dimension_2d; // 2048
        // let atlas_size = 256;

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

        let mask_vertex_buffer_size = 4096 * 9;
        let mask_vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vertices"),
            size: mask_vertex_buffer_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // let color_texture = device.create_texture(&TextureDescriptor {
        //     label: Some("atlas"),
        //     size: Extent3d {
        //         width: atlas_size,
        //         height: atlas_size,
        //         depth_or_array_layers: 1,
        //     },
        //     mip_level_count: 1,
        //     sample_count: 1,
        //     dimension: TextureDimension::D2,
        //     format: TextureFormat::Rgba8Unorm,
        //     usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        //     view_formats: &[],
        // });
        // let color_texture_view = color_texture.create_view(&TextureViewDescriptor::default());

        // let color_vertex_buffer_size = 4096 * 9;
        // let color_vertex_buffer = device.create_buffer(&BufferDescriptor {
        //     label: Some("vertices"),
        //     size: color_vertex_buffer_size,
        //     usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        //     mapped_at_creation: false,
        // });

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

        let atlas_bind_group_layout = device.create_bind_group_layout(&ATLAS_BIND_GROUP_LAYOUT);

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
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

        let mask_atlas = Atlas::<GrayImage> {
            glyph_cache: LruCache::unbounded_with_hasher(BuildHasherDefault::<FxHasher>::default()),
            pages: vec![AtlasPage::<GrayImage> {
                image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
                last_frame_evicted: 0,
                packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
                quads: Vec::<Quad>::with_capacity(300),
                vertex_buffer_size: mask_vertex_buffer_size,
                gpu: Some(GpuAtlasPage {
                    texture: mask_texture,
                    vertex_buffer: mask_vertex_buffer,
                    bind_group: bind_group,
                })
            }],
        };

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&atlas_bind_group_layout, &uniforms_layout],
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

        let tmp_image = Image::new();
        let font_cx = FontContext::new();
        let layout_cx = LayoutContext::<ColorBrush>::new();
        let frame = 1;
        Self { frame, atlas_size, tmp_image, font_cx, layout_cx, mask_atlas, pipeline, atlas_bind_group_layout, sampler, params, params_buffer, params_bind_group, }
    }
}