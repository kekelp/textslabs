use crate::*;

pub(crate) const INITIAL_BUFFER_SIZE: u64 = 4096;


const ATLAS_BIND_GROUP_LAYOUT_DESC: BindGroupLayoutDescriptor = wgpu::BindGroupLayoutDescriptor {
    entries: &[
        BindGroupLayoutEntry {
            binding: 0,
            // This is visible in vertex to get the size. Could be avoided, but I don't know if it's a big deal.
            visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
            ty: BindingType::Texture {
                multisampled: false,
                view_dimension: TextureViewDimension::D2Array,
                sample_type: TextureSampleType::Float { filterable: true },
            },
            count: None,
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                multisampled: false,
                view_dimension: TextureViewDimension::D2Array,
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
        // Experimentally bind the vertex buffer as well
        wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    ],
    label: Some("atlas bind group layout"),
};

/// Configuration parameters for the text renderer.
pub struct TextRendererParams {
    /// Size of texture atlas pages used for glyph caching.
    pub atlas_page_size: AtlasPageSize,
    /// Enable z-range filtering using push constants. Required for render_z_range().
    pub enable_z_range_filtering: bool,
}
impl Default for TextRendererParams {
    fn default() -> Self {
        // 2048 is guaranteed to work everywhere that webgpu supports, and it seems both small enough that it's fine to allocate it upfront even if a smaller one would have been fine, and big enough that even on gpus that could hold 8k textures, I don't feel too bad about using multiple 2k pages instead of a single big 8k one
        // Ideally you'd still with small pages and grow them until the max texture dim, but having cache eviction, multiple pages, AND page growing seems a bit too much for now
        let atlas_page_size = AtlasPageSize::DownlevelWrbgl2Max; // 2048
        Self { 
            atlas_page_size,
            enable_z_range_filtering: false,
        }
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

pub(crate) fn create_vertex_buffer(device: &Device, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("shared vertex buffer"),
        size,
        usage: BufferUsages::VERTEX | BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn generate_shader_source(enable_z_range_filtering: bool) -> ShaderSource<'static> {
    if enable_z_range_filtering {
        let base_shader = include_str!("shader.wgsl");
        let push_constants_template = include_str!("push_constants.wgsl");
        let z_range_filter_template = include_str!("z_range_filter.wgsl");
        
        // Add push constants after params declaration
        let shader_with_push_constants = base_shader.replace(
            "@group(1) @binding(0)\nvar<uniform> params: Params;",
            &format!("@group(1) @binding(0)\nvar<uniform> params: Params;\n\n{}", push_constants_template)
        );
        
        // Replace the vertex shader function with the z-range filtering version
        let final_shader = shader_with_push_constants.replace(
            "@vertex\nfn vs_main(input: VertexInput) -> VertexOutput {\n    var vert_output: VertexOutput;",
            z_range_filter_template
        );
        
        ShaderSource::Wgsl(Cow::Owned(final_shader))
    } else {
        ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl")))
    }
}

impl ContextlessTextRenderer {
    pub fn new_with_params(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        depth_stencil: Option<DepthStencilState>,
        params: TextRendererParams,
    ) -> Self {
        let srgb = format.is_srgb();
        
        let atlas_size = params.atlas_page_size.size(device);


        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("sampler"),
            min_filter: FilterMode::Nearest,
            mag_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            lod_min_clamp: 0f32,
            lod_max_clamp: 0f32,
            ..Default::default()
        });

        let shader_source = generate_shader_source(params.enable_z_range_filtering);
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("shader"),
            source: shader_source,
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
                7 => Uint32,
            ],
        };

        let uniform_params = Params {
            screen_resolution_width: 0.0,
            screen_resolution_height: 0.0,
            srgb: if srgb { 1 } else { 0 },
            _pad: 0,
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
                visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
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

        let atlas_bind_group_layout = device.create_bind_group_layout(&ATLAS_BIND_GROUP_LAYOUT_DESC);

        let glyph_cache = LruCache::unbounded_with_hasher(BuildHasherDefault::<FxHasher>::default());

        let mask_atlas_pages = vec![AtlasPage {
            image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
            packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
        }];
        
        let color_atlas_pages = vec![AtlasPage {
            image: RgbaImage::from_pixel(atlas_size, atlas_size, Rgba([0, 0, 0, 0])),
            packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
        }];

        let push_constant_ranges = if params.enable_z_range_filtering {
            vec![wgpu::PushConstantRange {
                stages: wgpu::ShaderStages::VERTEX,
                range: 0..8, // vec2<f32> = 8 bytes
            }]
        } else {
            vec![]
        };

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&atlas_bind_group_layout, &params_layout],
            push_constant_ranges: &push_constant_ranges,
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
        
        let (mask_texture_array, color_texture_array) = rebuild_texture_arrays(
            device,
            queue,
            atlas_size,
            &mask_atlas_pages,
            &color_atlas_pages,
        );

        let atlas_bind_group = create_atlas_bind_group(
            device,
            &mask_texture_array,
            &color_texture_array,
            &vertex_buffer,
            &sampler,
            &atlas_bind_group_layout,
        );

        return Self {
            frame,
            atlas_size,
            tmp_image,
            mask_atlas_pages,
            color_atlas_pages,
            quads: Vec::with_capacity(1000),
            mask_texture_array,
            color_texture_array,
            atlas_bind_group,
            pipeline,
            atlas_bind_group_layout,
            params_layout,
            sampler,
            params: uniform_params,
            params_buffer,
            params_bind_group,
            glyph_cache,
            last_frame_evicted: 0,
            z_range_filtering_enabled: params.enable_z_range_filtering,
            // cached_scaler: None,
            vertex_buffer,
            needs_gpu_sync: true,
            needs_texture_array_rebuild: false,
        };
    }
}

impl ContextlessTextRenderer {
    pub(crate) fn rebuild_texture_arrays(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let (mask_texture_array, color_texture_array) = rebuild_texture_arrays(
            device,
            queue,
            self.atlas_size,
            &self.mask_atlas_pages,
            &self.color_atlas_pages,
        );

        self.mask_texture_array = mask_texture_array;
        self.color_texture_array = color_texture_array;

        // Rebuild bind group after textures are updated
        self.create_atlas_bind_group(device);
    }

    pub(crate) fn update_texture_arrays(&mut self, queue: &wgpu::Queue) {
        // Update mask texture array
        for (i, page) in self.mask_atlas_pages.iter().enumerate() {
            queue.write_texture(
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
        }
    
        // Update color texture array
        for (i, page) in self.color_atlas_pages.iter().enumerate() {
            queue.write_texture(
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
        }
    }
    

    fn create_atlas_bind_group(&mut self, device: &wgpu::Device) {
        let bind_group = create_atlas_bind_group(
            device,
            &self.mask_texture_array,
            &self.color_texture_array,
            &self.vertex_buffer,
            &self.sampler,
            &self.atlas_bind_group_layout,
        );

        self.atlas_bind_group = bind_group;
    }
}


fn create_atlas_bind_group(
    device: &wgpu::Device,
    mask_texture_array: &wgpu::Texture,
    color_texture_array: &wgpu::Texture,
    vertex_buffer: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    atlas_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::BindGroup {

    let mask_view = mask_texture_array.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });

    let color_view = color_texture_array.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });

    let entries = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&mask_view),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(&color_view),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::Sampler(sampler),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: vertex_buffer.as_entire_binding(),
        },
    ];

    device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: atlas_bind_group_layout,
        entries: &entries,
        label: Some("atlas texture array bind group"),
    })
}

fn rebuild_texture_arrays(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas_size: u32,
    mask_atlas_pages: &[AtlasPage<GrayImage>],
    color_atlas_pages: &[AtlasPage<RgbaImage>],
) -> (wgpu::Texture, wgpu::Texture) {
    // Create mask texture array
    let mask_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mask atlas array"),
        size: wgpu::Extent3d {
            width: atlas_size,
            height: atlas_size,
            depth_or_array_layers: mask_atlas_pages.len() as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (i, page) in mask_atlas_pages.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &mask_tex,
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
    }

    // Create color texture array
    let color_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("color atlas array"),
        size: wgpu::Extent3d {
            width: atlas_size,
            height: atlas_size,
            depth_or_array_layers: color_atlas_pages.len() as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (i, page) in color_atlas_pages.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &color_tex,
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
    }

    (mask_tex, color_tex)
}
