use bytemuck::{Pod, Zeroable};
use glam::{DVec3, Mat4, Vec3};
use oldmine_common::world::{block_definition, generate_chunk, BlockId, CHUNK_SIZE, WORLD_HEIGHT};
use std::{collections::{HashMap, HashSet}, sync::Arc, time::Instant};
use std::sync::mpsc::{self, Receiver};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};
mod network;
mod input;
mod state;
mod world_client;
mod player;
mod texture_animation;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_projection: [[f32; 4]; 4],
    time: [f32; 4],
}

#[derive(Default)]
struct ChunkMesh {
    opaque: Vec<Vertex>,
    translucent: Vec<(f32, Vec<Vertex>)>,
    water: Vec<(f32, Vec<Vertex>)>,
}

struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    celestial_pipeline: wgpu::RenderPipeline,
    cloud_pipeline: wgpu::RenderPipeline,
    water_pipeline: wgpu::RenderPipeline,
    ui_pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    water_buffer: wgpu::Buffer,
    water_count: u32,
    opaque_buffer: wgpu::Buffer,
    sky_buffer: wgpu::Buffer,
    sky_count: u32,
    sun_buffer: wgpu::Buffer,
    moon_buffer: wgpu::Buffer,
    cloud_buffer: wgpu::Buffer,
    cloud_count: u32,
    celestial_bind_groups: Vec<wgpu::BindGroup>,
    cloud_bind_group: wgpu::BindGroup,
    water_bind_group: wgpu::BindGroup,
    opaque_count: u32,
    translucent_buffer: wgpu::Buffer,
    translucent_count: u32,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    texture_bind_group: wgpu::BindGroup,
    terrain_texture: wgpu::Texture,
    water_fx: texture_animation::WaterFx,
    water_texture: wgpu::Texture,
    last_water_fx_tick: Instant,
    lava_still_fx: texture_animation::LavaFx,
    lava_flow_fx: texture_animation::LavaFx,
    last_lava_fx_tick: Instant,
    adapter_info: wgpu::AdapterInfo,
    ui_buffer: wgpu::Buffer,
    ui_count: u32,
    fps_sample_started: Instant,
    fps_sample_frames: u32,
    fps: u32,
    total_frames: u64,
    started_at: Instant,
    chunk_meshes: HashMap<(i32, i32), ChunkMesh>,
}

impl Renderer {
    async fn new(window: Arc<Window>) -> Result<Self, Box<dyn std::error::Error>> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no compatible graphics adapter found")?;
        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("oldmine device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            }, None)
            .await?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera uniform"),
            contents: bytemuck::bytes_of(&CameraUniform {
                view_projection: Mat4::IDENTITY.to_cols_array_2d(),
                time: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let (terrain_texture, texture_view, texture_sampler) = load_terrain_texture(&device, &queue)?;
        let mut water_fx = texture_animation::WaterFx::default();
        water_fx.tick();
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain texture bind group"), layout: &texture_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&texture_sampler) }],
        });
        let (water_texture, water_view, water_sampler) = load_water_texture(&device, &queue, water_fx.pixels())?;
        let water_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dedicated Beta water textures"), layout: &texture_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&water_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&water_sampler) }],
        });
        let mut lava_still_fx = texture_animation::LavaFx::still();
        let mut lava_flow_fx = texture_animation::LavaFx::flowing();
        // RenderEngine.registerTextureFX calls onTick immediately.
        lava_still_fx.tick();
        lava_flow_fx.tick();
        upload_lava_fx(&queue, &terrain_texture, lava_still_fx.pixels(), 237, lava_still_fx.tile_size());
        upload_lava_fx(&queue, &terrain_texture, lava_flow_fx.pixels(), 238, lava_flow_fx.tile_size());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voxel pipeline layout"),
            bind_group_layouts: &[&camera_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("voxel pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("beta sky shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("beta sky pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("beta sky layout"), bind_group_layouts: &[&camera_layout], push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState { module: &sky_shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Vertex::layout()] },
            fragment: Some(wgpu::FragmentState { module: &sky_shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth24Plus, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::Always, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview: None, cache: None,
        });
        let (sun_view, sun_sampler) = load_png_texture(&device, &queue, include_bytes!("../assets/terrain/sun.png"), "sun.png")?;
        let (moon_view, moon_sampler) = load_png_texture(&device, &queue, include_bytes!("../assets/terrain/moon.png"), "moon.png")?;
        let (cloud_view, cloud_sampler) = load_png_texture(&device, &queue, include_bytes!("../assets/environment/clouds.png"), "clouds.png")?;
        let celestial_bind_groups = vec![
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sun bind group"), layout: &texture_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sun_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sun_sampler) }] }),
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("moon bind group"), layout: &texture_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&moon_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&moon_sampler) }] }),
        ];
        let celestial_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("celestial shader"), source: wgpu::ShaderSource::Wgsl(include_str!("celestial.wgsl").into()) });
        let celestial_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("celestial pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("celestial layout"), bind_group_layouts: &[&camera_layout, &texture_layout], push_constant_ranges: &[] })),
            vertex: wgpu::VertexState { module: &celestial_shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Vertex::layout()] },
            fragment: Some(wgpu::FragmentState { module: &celestial_shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add } }), write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth24Plus, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::Always, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview: None, cache: None,
        });
        let cloud_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("beta cloud shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cloud.wgsl").into()),
        });
        let cloud_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cloud pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("cloud layout"), bind_group_layouts: &[&camera_layout, &texture_layout], push_constant_ranges: &[] })),
            vertex: wgpu::VertexState { module: &cloud_shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Vertex::layout()] },
            fragment: Some(wgpu::FragmentState { module: &cloud_shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth24Plus, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview: None, cache: None,
        });
        let cloud_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("cloud bind group"), layout: &texture_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&cloud_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&cloud_sampler) }] });
        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("debug HUD shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("debug HUD pipeline"),
            layout: None,
            vertex: wgpu::VertexState { module: &ui_shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Vertex::layout()] },
            fragment: Some(wgpu::FragmentState { module: &ui_shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(), multiview: None, cache: None,
        });
        let translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("translucent voxel pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Vertex::layout()] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth24Plus, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview: None, cache: None,
        });
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dedicated water pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Vertex::layout()] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth24Plus, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview: None, cache: None,
        });

        let empty = [Vertex { position: [0.0, 0.0, 0.0], uv: [0.0, 0.0], color: [0.0, 0.0, 0.0, 0.0] }];
        let opaque_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("opaque mesh"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });
        let translucent_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("translucent mesh"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });
        let water_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("water mesh"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });
        let sky_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("sky mesh"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });
        let sun_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("sun mesh"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });
        let moon_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("moon mesh"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });
        let cloud_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("cloud mesh"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });
        let ui_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("debug HUD vertices"), contents: bytemuck::cast_slice(&empty), usage: wgpu::BufferUsages::VERTEX });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            sky_pipeline,
            celestial_pipeline,
            cloud_pipeline,
            water_pipeline,
            ui_pipeline,
            translucent_pipeline,
            opaque_buffer,
            sky_buffer,
            sky_count: 0,
            sun_buffer,
            moon_buffer,
            cloud_buffer,
            cloud_count: 0,
            celestial_bind_groups,
            cloud_bind_group,
            opaque_count: 0,
            translucent_buffer,
            translucent_count: 0,
            water_buffer,
            water_count: 0,
            camera_buffer,
            camera_bind_group,
            texture_bind_group,
            terrain_texture,
            water_bind_group,
            water_fx,
            water_texture,
            last_water_fx_tick: Instant::now(),
            lava_still_fx,
            lava_flow_fx,
            last_lava_fx_tick: Instant::now(),
            adapter_info,
            ui_buffer,
            ui_count: 0,
            fps_sample_started: Instant::now(),
            fps_sample_frames: 0,
            fps: 0,
            total_frames: 0,
            started_at: Instant::now(),
            chunk_meshes: HashMap::new(),
        })
    }

    fn rebuild_mesh(&mut self, world: &world_client::WorldClient, camera_position: Vec3) {
        self.chunk_meshes.clear();
        let (opaque, translucent, water) = build_world_mesh(world, camera_position);
        self.chunk_meshes.insert((0, 0), ChunkMesh { opaque, translucent: vec![(0.0, translucent)], water: vec![(0.0, water)] });
        self.upload_cached_meshes();
    }

    fn rebuild_dirty_meshes(&mut self, world: &mut world_client::WorldClient, camera_position: Vec3) {
        let dirty = world.take_dirty();
        if !dirty.is_empty() { self.rebuild_mesh(world, camera_position); }
    }

    fn upload_cached_meshes(&mut self) {
        let mut opaque = Vec::new();
        let mut translucent = Vec::new();
        let mut water = Vec::new();
        for mesh in self.chunk_meshes.values() {
            opaque.extend_from_slice(&mesh.opaque);
            translucent.extend(mesh.translucent.iter().cloned());
            water.extend(mesh.water.iter().cloned());
        }
        translucent.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(std::cmp::Ordering::Equal));
        let translucent: Vec<Vertex> = translucent.into_iter().flat_map(|(_, face)| face).collect();
        water.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(std::cmp::Ordering::Equal));
        let water: Vec<Vertex> = water.into_iter().flat_map(|(_, face)| face).collect();
        self.opaque_count = opaque.len() as u32;
        self.translucent_count = translucent.len() as u32;
        self.water_count = water.len() as u32;
        self.opaque_buffer = self.upload_vertices("opaque world mesh", &opaque);
        self.translucent_buffer = self.upload_vertices("translucent world mesh", &translucent);
        self.water_buffer = self.upload_vertices("dedicated water world mesh", &water);
    }

    fn rebuild_translucent(&mut self, world: &world_client::WorldClient, camera_position: Vec3) {
        let (_, translucent, water) = build_world_mesh(world, camera_position);
        let mut translucent = vec![(0.0, translucent)];
        translucent.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(std::cmp::Ordering::Equal));
        let translucent: Vec<Vertex> = translucent.into_iter().flat_map(|(_, face)| face).collect();
        self.translucent_count = translucent.len() as u32;
        self.translucent_buffer = self.upload_vertices("translucent camera sort", &translucent);
        let mut water = vec![(0.0, water)];
        water.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(std::cmp::Ordering::Equal));
        let water: Vec<Vertex> = water.into_iter().flat_map(|(_, face)| face).collect();
        self.water_count = water.len() as u32;
        self.water_buffer = self.upload_vertices("dedicated water camera sort", &water);
    }

    fn upload_vertices(&self, label: &str, vertices: &[Vertex]) -> wgpu::Buffer {
        let empty = [Vertex { position: [0.0, 0.0, 0.0], uv: [0.0, 0.0], color: [0.0, 0.0, 0.0, 0.0] }];
        let bytes = if vertices.is_empty() { bytemuck::cast_slice(&empty) } else { bytemuck::cast_slice(vertices) };
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage: wgpu::BufferUsages::VERTEX })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self, camera: &Camera, entities: &std::collections::HashMap<i32, state::RemoteEntity>, world_time: f32, show_debug: bool, render_debug: bool, loaded_chunks: usize) -> Result<(), wgpu::SurfaceError> {
        while self.last_water_fx_tick.elapsed() >= std::time::Duration::from_millis(50) {
            self.water_fx.tick();
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &self.water_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                self.water_fx.pixels(),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(16 * 4), rows_per_image: Some(16) },
                wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            );
            self.last_water_fx_tick += std::time::Duration::from_millis(50);
        }
        while self.last_lava_fx_tick.elapsed() >= std::time::Duration::from_millis(50) {
            self.lava_still_fx.tick();
            self.lava_flow_fx.tick();
            upload_lava_fx(&self.queue, &self.terrain_texture, self.lava_still_fx.pixels(), 237, self.lava_still_fx.tile_size());
            upload_lava_fx(&self.queue, &self.terrain_texture, self.lava_flow_fx.pixels(), 238, self.lava_flow_fx.tile_size());
            self.last_lava_fx_tick += std::time::Duration::from_millis(50);
        }
        self.total_frames += 1;
        self.fps_sample_frames += 1;
        let sample_elapsed = self.fps_sample_started.elapsed();
        if sample_elapsed >= std::time::Duration::from_millis(500) {
            self.fps = (self.fps_sample_frames as f32 / sample_elapsed.as_secs_f32()).round() as u32;
            self.fps_sample_frames = 0;
            self.fps_sample_started = Instant::now();
        }
        let view = camera.view_projection(
            self.config.width as f32 / self.config.height as f32,
        );
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform {
                view_projection: view.to_cols_array_2d(),
                time: [self.started_at.elapsed().as_secs_f32(), 0.0, 0.0, 0.0],
            }),
        );
        let entity_vertices = build_entity_mesh(entities);
        let entity_buffer = self.upload_vertices("remote entities", &entity_vertices);
        let sky_vertices = build_sky_mesh(camera.position, world_time);
        self.sky_count = sky_vertices.len() as u32;
        self.sky_buffer = self.upload_vertices("beta sky", &sky_vertices);
        let (sun_vertices, moon_vertices) = build_celestial_mesh(camera.position, world_time);
        self.sun_buffer = self.upload_vertices("sun", &sun_vertices);
        self.moon_buffer = self.upload_vertices("moon", &moon_vertices);
        let cloud_vertices = build_cloud_mesh(camera.position, world_time, self.started_at.elapsed().as_secs_f32());
        self.cloud_count = cloud_vertices.len() as u32;
        self.cloud_buffer = self.upload_vertices("clouds", &cloud_vertices);
        let sky_daylight = (f32::cos(celestial_angle(world_time) * std::f32::consts::TAU) * 2.0 + 0.5).clamp(0.0, 1.0);
        let sky_horizon = [0.72 * sky_daylight, 0.86 * sky_daylight, sky_daylight];
        if show_debug {
            let debug_vertices = build_debug_hud(self, camera, world_time, render_debug, loaded_chunks, entities.len());
            self.ui_count = debug_vertices.len() as u32;
            self.ui_buffer = self.upload_vertices("debug HUD", &debug_vertices);
        } else {
            self.ui_count = 0;
        }
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&Default::default());
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth buffer"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky_horizon[0] as f64,
                            g: sky_horizon[1] as f64,
                            b: sky_horizon[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_vertex_buffer(0, self.sky_buffer.slice(..));
            pass.draw(0..self.sky_count, 0..1);
            pass.set_pipeline(&self.celestial_pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, &self.celestial_bind_groups[0], &[]);
            pass.set_vertex_buffer(0, self.sun_buffer.slice(..));
            pass.draw(0..6, 0..1);
            pass.set_bind_group(1, &self.celestial_bind_groups[1], &[]);
            pass.set_vertex_buffer(0, self.moon_buffer.slice(..));
            pass.draw(0..6, 0..1);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, &self.texture_bind_group, &[]);
            pass.set_vertex_buffer(0, self.opaque_buffer.slice(..));
            if self.opaque_count > 0 { pass.draw(0..self.opaque_count, 0..1); }
            pass.set_pipeline(&self.cloud_pipeline);
            pass.set_bind_group(1, &self.cloud_bind_group, &[]);
            pass.set_vertex_buffer(0, self.cloud_buffer.slice(..));
            pass.draw(0..self.cloud_count, 0..1);
            if !entity_vertices.is_empty() { pass.set_pipeline(&self.pipeline); pass.set_bind_group(1, &self.texture_bind_group, &[]); pass.set_vertex_buffer(0, entity_buffer.slice(..)); pass.draw(0..entity_vertices.len() as u32, 0..1); }
            if self.translucent_count > 0 {
                pass.set_pipeline(&self.translucent_pipeline);
                pass.set_bind_group(1, &self.texture_bind_group, &[]);
                pass.set_vertex_buffer(0, self.translucent_buffer.slice(..));
                pass.draw(0..self.translucent_count, 0..1);
            }
            if self.water_count > 0 {
                pass.set_pipeline(&self.water_pipeline);
                pass.set_bind_group(1, &self.water_bind_group, &[]);
                pass.set_vertex_buffer(0, self.water_buffer.slice(..));
                pass.draw(0..self.water_count, 0..1);
            }
        }
        if self.ui_count > 0 {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("debug HUD pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
            pass.draw(0..self.ui_count, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }
}

struct Camera {
    position: Vec3,
    yaw: f32,
    pitch: f32,
}

fn build_debug_hud(
    renderer: &Renderer,
    camera: &Camera,
    world_time: f32,
    render_debug: bool,
    loaded_chunks: usize,
    entity_count: usize,
) -> Vec<Vertex> {
    const SCALE: f32 = 2.0;
    const LINE_HEIGHT: f32 = 17.0;
    const PADDING: f32 = 8.0;
    let frame_ms = if renderer.fps > 0 { 1000.0 / renderer.fps as f32 } else { 0.0 };
    let info = &renderer.adapter_info;
    let lines = vec![
        format!("OLDMINE DEBUG  |  F3 TOGGLE  |  FRAME {}", renderer.total_frames),
        format!("FPS: {}  |  FRAME TIME: {:.1} MS", renderer.fps, frame_ms),
        format!("BACKEND: {:?}  |  DEVICE: {:?}", info.backend, info.device_type),
        format!("ADAPTER: {}", info.name),
        format!("DRIVER: {} {}", info.driver, info.driver_info),
        format!("RES: {} X {}  |  FORMAT: {:?}", renderer.config.width, renderer.config.height, renderer.config.format),
        format!("CAM: X {:.2}  Y {:.2}  Z {:.2}", camera.position.x, camera.position.y, camera.position.z),
        format!("ROT: YAW {:.1}  PITCH {:.1}", camera.yaw.to_degrees(), camera.pitch.to_degrees()),
        format!("WORLD TICK: {:.0}  |  CHUNKS: {}  |  ENTITIES: {}", world_time, loaded_chunks, entity_count),
        format!("MESH VERTICES: OPAQUE {}  TRANS {}  WATER {}  SKY {}", renderer.opaque_count, renderer.translucent_count, renderer.water_count, renderer.sky_count),
        format!("WATER FX TICKS: {}  |  ATLAS TILE: 205", renderer.water_fx.ticks),
        format!("LAVA FX: STILL {}  FLOW {}  |  ATLAS TILES: 237-238", renderer.lava_still_fx.ticks, renderer.lava_flow_fx.ticks),
        format!("MODE: {}  |  RENDER PASS: TERRAIN / SKY / CELESTIAL / CLOUD / TRANS", if render_debug { "RENDER DEBUG" } else { "CLIENT" }),
    ];
    let max_chars = ((renderer.config.width as f32 - PADDING * 2.0 - 8.0) / (6.0 * SCALE)).max(1.0) as usize;
    let clipped: Vec<String> = lines.iter().map(|line| line.chars().take(max_chars).collect()).collect();
    let panel_width = clipped.iter().map(|line| line.len()).max().unwrap_or(0) as f32 * 6.0 * SCALE + PADDING * 2.0;
    let panel_height = clipped.len() as f32 * LINE_HEIGHT + PADDING * 2.0;
    let mut vertices = Vec::new();
    push_ui_rect(&mut vertices, 5.0, 5.0, panel_width, panel_height, [0.0, 0.0, 0.0, 0.68], renderer.config.width, renderer.config.height);
    for (row, line) in clipped.iter().enumerate() {
        draw_bitmap_text(
            &mut vertices,
            line,
            5.0 + PADDING,
            5.0 + PADDING + row as f32 * LINE_HEIGHT,
            SCALE,
            [1.0, 1.0, 1.0, 1.0],
            renderer.config.width,
            renderer.config.height,
        );
    }
    vertices
}

fn draw_bitmap_text(vertices: &mut Vec<Vertex>, text: &str, x: f32, y: f32, scale: f32, color: [f32; 4], width: u32, height: u32) {
    for (column, character) in text.chars().enumerate() {
        let glyph = debug_glyph(character.to_ascii_uppercase());
        for (row, bits) in glyph.into_iter().enumerate() {
            for pixel in 0..5 {
                if bits & (1 << (4 - pixel)) != 0 {
                    push_ui_rect(vertices, x + (column as f32 * 6.0 + pixel as f32) * scale, y + row as f32 * scale, scale, scale, color, width, height);
                }
            }
        }
    }
}

fn push_ui_rect(vertices: &mut Vec<Vertex>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], width: u32, height: u32) {
    let to_ndc = |px: f32, py: f32| [px / width as f32 * 2.0 - 1.0, 1.0 - py / height as f32 * 2.0, 0.0];
    let corners = [to_ndc(x, y), to_ndc(x + w, y), to_ndc(x + w, y + h), to_ndc(x, y + h)];
    for index in [0usize, 1, 2, 0, 2, 3] {
        vertices.push(Vertex { position: corners[index], uv: [0.0, 0.0], color });
    }
}

fn debug_glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [14, 17, 17, 31, 17, 17, 17], 'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14], 'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31], 'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15], 'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14], 'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17], 'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17], 'N' => [17, 25, 25, 21, 19, 19, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14], 'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13], 'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30], 'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14], 'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10], 'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4], 'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14], '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31], '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2], '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14], '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14], '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0], '.' => [0, 0, 0, 0, 0, 6, 6],
        ',' => [0, 0, 0, 0, 6, 6, 4], '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0], '/' => [1, 2, 2, 4, 8, 8, 16],
        '|' => [4, 4, 4, 4, 4, 4, 4], '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8], '=' => [0, 31, 0, 31, 0, 0, 0],
        _ => [0; 7],
    }
}

fn build_render_debug_water_fixture(world: &mut world_client::WorldClient) {
    const X_MIN: i32 = 3;
    const X_MAX: i32 = 12;
    const Z_MIN: i32 = -31;
    const Z_MAX: i32 = -20;
    const FLOOR_Y: i32 = 61;
    const WATER_Y: i32 = 62;

    for x in X_MIN..=X_MAX {
        for z in Z_MIN..=Z_MAX {
            for y in (WATER_Y..=75).rev() {
                world.set_block(x, y, z, BlockId::Air as u8);
            }
            world.set_block(x, FLOOR_Y, z, BlockId::Stone as u8);
            let border = x == X_MIN || x == X_MAX || z == Z_MIN || z == Z_MAX;
            if border {
                world.set_block(x, WATER_Y, z, 4);
            } else if x < 8 {
                world.set_block_state(x, WATER_Y, z, BlockId::Water as u8, 0);
            } else {
                world.set_block_state(x, WATER_Y, z, 8, 1);
            }
            for y in FLOOR_Y..=WATER_Y + 1 {
                world.set_light(x, y, z, 0, 15);
            }
        }
    }
}

impl Camera {
    fn view_projection(&self, aspect: f32) -> Mat4 {
        let direction = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        Mat4::perspective_rh(70_f32.to_radians(), aspect, 0.1, 512.0)
            * Mat4::look_to_rh(self.position, direction, Vec3::Y)
    }

    fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin()).normalize()
    }
}

#[derive(Default)]
struct ClientApp {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    keys: HashSet<KeyCode>,
    camera: Option<Camera>,
    last_frame: Option<Instant>,
    mouse_locked: bool,
    server_spawn: Option<(i32, i32, i32)>,
    render_debug: bool,
    debug_overlay: bool,
    client_state: state::ClientState,
    network_rx: Option<Receiver<state::NetworkEvent>>,
}

impl ApplicationHandler for ClientApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Oldmine Rust Client")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        match event_loop.create_window(attrs) {
            Ok(window) => {
                let window = Arc::new(window);
                match pollster::block_on(Renderer::new(window.clone())) {
                    Ok(renderer) => {
                        self.renderer = Some(renderer);
                        let render_debug = std::env::var("OLDMINE_RENDER_DEBUG").map(|value| value != "0").unwrap_or(false);
                        self.render_debug = render_debug;
                        self.debug_overlay = render_debug;
                        if render_debug {
                            // Keep the renderer and physics on solid ground before any server packets arrive.
                            for chunk_x in -2..=2 {
                                for chunk_z in -2..=2 {
                                    self.client_state.world.load_chunk(generate_chunk(0x0DDBA11, chunk_x, chunk_z));
                                }
                            }
                            build_render_debug_water_fixture(&mut self.client_state.world);
                            self.client_state.phase = state::ClientPhase::Playing;
                            self.client_state.player.position = Vec3::new(8.0, 64.0, -16.0);
                            self.client_state.player.on_ground = true;
                            eprintln!("render debug world loaded: 5x5 generated chunks");
                        } else {
                            self.client_state.phase = state::ClientPhase::Connecting;
                            let (tx, rx) = mpsc::channel();
                            self.network_rx = Some(rx);
                            std::thread::spawn(move || {
                                let event = match network::BetaConnection::connect("127.0.0.1:25565", "RustPlayer") {
                                    Ok(connection) => connection.run(tx.clone()),
                                    Err(error) => state::NetworkEvent::Failed { reason: error.to_string() },
                                };
                                if let state::NetworkEvent::Failed { .. } = event { let _ = tx.send(event); }
                            });
                        }
                        self.camera = Some(Camera {
                            position: if render_debug { Vec3::new(8.0, 65.62, -16.0) } else { Vec3::new(8.0, 72.0, 28.0) },
                            yaw: if render_debug { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 },
                            pitch: if render_debug { -0.12 } else { -0.2 },
                        });
                        if render_debug {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.rebuild_dirty_meshes(&mut self.client_state.world, Vec3::new(8.0, 65.62, -16.0));
                            }
                        }
                        window.request_redraw();
                        self.window = Some(window);
                    }
                    Err(error) => {
                        eprintln!("failed to initialize graphics: {error}");
                        event_loop.exit();
                    }
                }
            }
            Err(error) => {
                eprintln!("failed to create client window: {error}");
                event_loop.exit();
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: winit::event::DeviceId, event: DeviceEvent) {
        if self.mouse_locked {
            if let DeviceEvent::MouseMotion { delta } = event {
                if let Some(camera) = &mut self.camera {
                    camera.yaw += delta.0 as f32 * 0.0025;
                    camera.pitch = (camera.pitch - delta.1 as f32 * 0.0025)
                        .clamp(-1.54, 1.54);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            let already_down = !self.keys.insert(code);
                            if code == KeyCode::F3 && !already_down { self.debug_overlay = !self.debug_overlay; }
                        }
                        ElementState::Released => { self.keys.remove(&code); }
                    }
                    if code == KeyCode::Escape && event.state == ElementState::Pressed {
                        self.mouse_locked = false;
                        if let Some(window) = &self.window {
                            let _ = window.set_cursor_grab(CursorGrabMode::None);
                            window.set_cursor_visible(true);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { button: MouseButton::Left, state: ElementState::Pressed, .. } => {
                if let Some(window) = &self.window {
                    let locked = window.set_cursor_grab(CursorGrabMode::Locked)
                        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                    if locked.is_ok() {
                        window.set_cursor_visible(false);
                        self.mouse_locked = true;
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let mut world_changed = false;
                let mut camera_moved = false;
                if let Some(rx) = &self.network_rx {
                    while let Ok(event) = rx.try_recv() {
                        if let state::NetworkEvent::Connected { spawn } = event {
                            self.server_spawn = Some(spawn);
                            eprintln!("connected to Beta server at spawn {spawn:?}");
                            self.client_state.apply_network_event(state::NetworkEvent::Connected { spawn });
                        } else if let state::NetworkEvent::Failed { reason } = event {
                            eprintln!("server connection unavailable: {reason}");
                            self.client_state.apply_network_event(state::NetworkEvent::Failed { reason });
                        } else {
                            self.client_state.apply_network_event(event);
                            world_changed = true;
                        }
                    }
                }
                if world_changed {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.rebuild_dirty_meshes(&mut self.client_state.world, self.camera.as_ref().map(|camera| camera.position).unwrap_or(Vec3::ZERO));
                    }
                }
                let now = Instant::now();
                let dt = self.last_frame.replace(now)
                    .map(|last| now.duration_since(last).as_secs_f32().min(0.05))
                    .unwrap_or(0.0);
                self.client_state.tick(dt);
                if let Some(camera) = &mut self.camera {
                    let previous_position = camera.position;
                    let forward = if self.keys.contains(&KeyCode::KeyW) { 1.0 } else if self.keys.contains(&KeyCode::KeyS) { -1.0 } else { 0.0 };
                    let strafe = if self.keys.contains(&KeyCode::KeyA) { 1.0 } else if self.keys.contains(&KeyCode::KeyD) { -1.0 } else { 0.0 };
                    self.client_state.player.yaw = camera.yaw;
                    if !self.render_debug {
                        self.client_state.player.tick(&self.client_state.world, forward, strafe, self.keys.contains(&KeyCode::Space), dt);
                    }
                    camera.position = self.client_state.player.position + Vec3::new(0.0, 1.62, 0.0);
                    camera_moved = camera.position.distance_squared(previous_position) > 0.04;
                }
                if camera_moved && !world_changed {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.rebuild_translucent(&self.client_state.world, self.camera.as_ref().map(|camera| camera.position).unwrap_or(Vec3::ZERO));
                    }
                }
                if let (Some(renderer), Some(camera)) = (&mut self.renderer, &self.camera) {
                    if let Err(error) = renderer.render(camera, &self.client_state.entities, self.client_state.world_time as f32, self.debug_overlay, self.render_debug, self.client_state.world.loaded_count()) {
                        match error {
                            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => {
                                if let Some(window) = &self.window {
                                    let size = window.inner_size();
                                    renderer.resize(size.width, size.height);
                                }
                            }
                            wgpu::SurfaceError::OutOfMemory => event_loop.exit(),
                            _ => {}
                        }
                    }
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn build_sky_mesh(camera: Vec3, world_time: f32) -> Vec<Vertex> {
    const GRID_STEP: f32 = 64.0;
    const GRID_CELLS: usize = 12;
    const GRID_HALF_EXTENT: f32 = GRID_STEP * GRID_CELLS as f32 * 0.5;

    let daylight = (f32::cos(celestial_angle(world_time) * std::f32::consts::TAU) * 2.0 + 0.5).clamp(0.0, 1.0);
    let sky_color = [0.24 * daylight, 0.52 * daylight, 0.92 * daylight, 1.0];
    let fog_color = [0.72 * daylight, 0.86 * daylight, daylight, 1.0];
    let fog_start = 0.0;
    let fog_end = 204.8;
    let mut vertices = Vec::with_capacity(GRID_CELLS * GRID_CELLS * 12);

    for (height, lower) in [(camera.y + 16.0, false), (camera.y - 16.0, true)] {
        for x in 0..GRID_CELLS {
            for z in 0..GRID_CELLS {
                let x0 = camera.x - GRID_HALF_EXTENT + x as f32 * GRID_STEP;
                let x1 = x0 + GRID_STEP;
                let z0 = camera.z - GRID_HALF_EXTENT + z as f32 * GRID_STEP;
                let z1 = z0 + GRID_STEP;
                let points = [[x0, height, z0], [x1, height, z0], [x1, height, z1], [x0, height, z1]];
                let colors = points.map(|point| {
                    let dx = point[0] - camera.x;
                    let dz = point[2] - camera.z;
                    let distance = (dx * dx + dz * dz).sqrt();
                    let fog = ((distance - fog_start) / (fog_end - fog_start)).clamp(0.0, 1.0);
                    [
                        sky_color[0] + (fog_color[0] - sky_color[0]) * fog,
                        sky_color[1] + (fog_color[1] - sky_color[1]) * fog,
                        sky_color[2] + (fog_color[2] - sky_color[2]) * fog,
                        1.0,
                    ]
                });
                let corners = if lower {
                    [points[1], points[0], points[3], points[2]]
                } else {
                    points
                };
                for index in [0usize, 1, 2, 0, 2, 3] {
                    let color_index = if lower { [1usize, 0, 3, 2][index] } else { index };
                    vertices.push(Vertex { position: corners[index], uv: [0.0, 0.0], color: colors[color_index] });
                }
            }
        }
    }
    vertices
}

fn celestial_angle(world_time: f32) -> f32 {
    let mut value = world_time.rem_euclid(24000.0) / 24000.0 - 0.25;
    if value < 0.0 { value += 1.0; }
    if value > 1.0 { value -= 1.0; }
    let original = value;
    value = 1.0 - (f32::cos(value * std::f32::consts::PI) + 1.0) * 0.5;
    original + (value - original) / 3.0
}

fn build_cloud_mesh(camera: Vec3, world_time: f32, elapsed_seconds: f32) -> Vec<Vertex> {
    const TILE: f32 = 32.0;
    const UV_SCALE: f32 = 0.5 / 1024.0;
    let cloud_x = (camera.x + elapsed_seconds * 20.0 * 0.03).rem_euclid(2048.0);
    let cloud_z = camera.z.rem_euclid(2048.0);
    let u_offset = cloud_x * UV_SCALE;
    let v_offset = cloud_z * UV_SCALE;
    let daylight = (f32::cos(celestial_angle(world_time) * std::f32::consts::TAU) * 2.0 + 0.5).clamp(0.0, 1.0);
    let tint = [0.92 * daylight + 0.08, 0.94 * daylight + 0.06, daylight, 0.8];
    let mut vertices = Vec::with_capacity(16 * 16 * 6);
    for x in (-256..256).step_by(32) {
        for z in (-256..256).step_by(32) {
            let x = x as f32;
            let z = z as f32;
            let quad = [
                ([x, 108.0, z + TILE], [x * UV_SCALE + u_offset, (z + TILE) * UV_SCALE + v_offset]),
                ([x + TILE, 108.0, z + TILE], [(x + TILE) * UV_SCALE + u_offset, (z + TILE) * UV_SCALE + v_offset]),
                ([x + TILE, 108.0, z], [(x + TILE) * UV_SCALE + u_offset, z * UV_SCALE + v_offset]),
                ([x, 108.0, z], [x * UV_SCALE + u_offset, z * UV_SCALE + v_offset]),
            ];
            for i in [0usize, 1, 2, 0, 2, 3] {
                let (p, uv) = quad[i];
                vertices.push(Vertex { position: [camera.x + p[0], p[1], camera.z + p[2]], uv, color: tint });
            }
        }
    }
    vertices
}

fn build_world_mesh(world: &world_client::WorldClient, camera_position: Vec3) -> (Vec<Vertex>, Vec<Vertex>, Vec<Vertex>) {
    let mut opaque = Vec::new();
    let mut translucent: Vec<(f32, Vec<Vertex>)> = Vec::new();
    let mut water: Vec<(f32, Vec<Vertex>)> = Vec::new();
    let faces: [([[f32; 3]; 4], (i32, i32, i32), bool); 6] = [
        ([[0.,0.,0.],[0.,0.,1.],[0.,1.,1.],[0.,1.,0.]], (-1,0,0), false),
        ([[1.,0.,1.],[1.,0.,0.],[1.,1.,0.],[1.,1.,1.]], (1,0,0), false),
        ([[0.,0.,0.],[1.,0.,0.],[1.,0.,1.],[0.,0.,1.]], (0,-1,0), false),
        ([[0.,1.,0.],[0.,1.,1.],[1.,1.,1.],[1.,1.,0.]], (0,1,0), true),
        ([[1.,0.,0.],[0.,0.,0.],[0.,1.,0.],[1.,1.,0.]], (0,0,-1), false),
        ([[0.,0.,1.],[1.,0.,1.],[1.,1.,1.],[0.,1.,1.]], (0,0,1), false),
    ];
    let triangles = [0, 1, 2, 0, 2, 3];
    for chunk in world.chunks() {
        for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            for y in 0..WORLD_HEIGHT {
                let Some(id) = chunk.block(x, y, z) else { continue };
                if id == BlockId::Air as u8 { continue; }
                let metadata = chunk.metadata(x, y, z).unwrap_or(0);
                let definition = block_definition(id);
                let (block_light, sky_light) = chunk.light(x, y, z).unwrap_or((0, 0));
                let brightness = (block_light.max(sky_light) as f32 / 15.0).max(0.35);
                if oldmine_common::world::block_is_crossed(id) {
                    let tile = definition.side_tiles[0] as u32;
                    let tx = (tile % 16) as f32 / 16.0;
                    let ty = (tile / 16) as f32 / 16.0;
                    let cx = chunk.x as f32 * 16.0 + x as f32;
                    let cz = chunk.z as f32 * 16.0 + z as f32;
                    for diagonal in 0..2 {
                        let corners = if diagonal == 0 { [[0.0,0.0,0.0],[1.0,0.0,1.0],[1.0,1.0,1.0],[0.0,1.0,0.0]] } else { [[1.0,0.0,0.0],[0.0,0.0,1.0],[0.0,1.0,1.0],[1.0,1.0,0.0]] };
                        let mut vertices = Vec::with_capacity(12);
                        for index in triangles {
                            let p = corners[index];
                            let uv = [[0.0,1.0],[1.0,1.0],[1.0,0.0],[0.0,0.0]][index];
                            vertices.push(Vertex { position: [cx+p[0], y as f32+p[1], cz+p[2]], uv: [tx+uv[0]/16.0, ty+uv[1]/16.0], color: [brightness, brightness, brightness, 1.0] });
                        }
                        opaque.extend(vertices);
                    }
                    continue;
                }
                if matches!(id, 27 | 28 | 66) {
                    let bx = chunk.x as f32 * 16.0 + x as f32;
                    let bz = chunk.z as f32 * 16.0 + z as f32;
                    let rail = build_rail_mesh(id, metadata, bx, y as f32, bz, brightness);
                    opaque.extend(rail);
                    continue;
                }
                if matches!(id, 8..=11) {
                    let world_x = chunk.x * CHUNK_SIZE as i32 + x as i32;
                    let world_z = chunk.z * CHUNK_SIZE as i32 + z as i32;
                    let center = Vec3::new(world_x as f32 + 0.5, y as f32 + 0.5, world_z as f32 + 0.5);
                    for mut face in build_fluid_faces(world, id, metadata, world_x, y as i32, world_z, brightness) {
                        if matches!(id, 8 | 9) {
                            remap_water_uv_to_dedicated_texture(&mut face);
                            water.push((center.distance_squared(camera_position), face));
                        } else {
                            // BlockFluid.getRenderBlockPass(): water is pass 1;
                            // lava remains in the opaque terrain pass (pass 0).
                            opaque.extend(face);
                        }
                    }
                    continue;
                }
                let boxes = if id == 85 {
                    oldmine_common::world::fence_boxes(metadata, world.block_at(chunk.x * 16 + x as i32, y as i32, chunk.z * 16 + z as i32 - 1) == 85, world.block_at(chunk.x * 16 + x as i32, y as i32, chunk.z * 16 + z as i32 + 1) == 85, world.block_at(chunk.x * 16 + x as i32 - 1, y as i32, chunk.z * 16 + z as i32) == 85, world.block_at(chunk.x * 16 + x as i32 + 1, y as i32, chunk.z * 16 + z as i32) == 85)
                } else {
                    oldmine_common::world::block_boxes(id, metadata)
                };
                if boxes.is_empty() { continue; }
                for bounds in boxes.iter().copied() {
                for (face_index, (corners, (dx, dy, dz), _is_top)) in faces.into_iter().enumerate() {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    let nz = z as i32 + dz;
                    let world_x = chunk.x * CHUNK_SIZE as i32 + nx;
                    let world_z = chunk.z * CHUNK_SIZE as i32 + nz;
                    let block_world_x = chunk.x * CHUNK_SIZE as i32 + x as i32;
                    let block_world_z = chunk.z * CHUNK_SIZE as i32 + z as i32;
                    if world.is_face_occluded(block_world_x, y as i32, block_world_z, face_index, bounds) { continue; }
                    if boxes.iter().any(|other| {
                        if *other == bounds { return false; }
                        match face_index {
                            0 => other[3] == bounds[0] && other[1] <= bounds[1] && other[4] >= bounds[4] && other[2] <= bounds[2] && other[5] >= bounds[5],
                            1 => other[0] == bounds[3] && other[1] <= bounds[1] && other[4] >= bounds[4] && other[2] <= bounds[2] && other[5] >= bounds[5],
                            2 => other[4] == bounds[1] && other[0] <= bounds[0] && other[3] >= bounds[3] && other[2] <= bounds[2] && other[5] >= bounds[5],
                            3 => other[1] == bounds[4] && other[0] <= bounds[0] && other[3] >= bounds[3] && other[2] <= bounds[2] && other[5] >= bounds[5],
                            4 => other[5] == bounds[2] && other[0] <= bounds[0] && other[3] >= bounds[3] && other[1] <= bounds[1] && other[4] >= bounds[4],
                            _ => other[2] == bounds[5] && other[0] <= bounds[0] && other[3] >= bounds[3] && other[1] <= bounds[1] && other[4] >= bounds[4],
                        }
                    }) { continue; }
                    let tile = oldmine_common::world::block_face_tile(id, metadata, face_index) as u32;
                    let tx = (tile % 16) as f32 / 16.0;
                    let ty = (tile / 16) as f32 / 16.0;
                    let mut face_vertices = Vec::with_capacity(6);
                    let clipped_corners = corners.map(|point| [
                        if point[0] == 0.0 { bounds[0] } else { bounds[3] },
                        if point[1] == 0.0 { bounds[1] } else { bounds[4] },
                        if point[2] == 0.0 { bounds[2] } else { bounds[5] },
                    ]);
                    for index in triangles {
                        let point = clipped_corners[index];
                        let corner_uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]][index];
                        face_vertices.push(Vertex {
                            position: [point[0] + x as f32 + chunk.x as f32 * 16.0, point[1] + y as f32, point[2] + z as f32 + chunk.z as f32 * 16.0],
                            uv: [tx + corner_uv[0] / 16.0, ty + corner_uv[1] / 16.0],
                            color: [brightness, brightness, brightness, if id == BlockId::Water as u8 { 0.72 } else if definition.translucent { 0.88 } else { 1.0 }],
                        });
                    }
                    if definition.translucent {
                        let center = Vec3::new(chunk.x as f32 * 16.0 + x as f32 + 0.5, y as f32 + 0.5, chunk.z as f32 * 16.0 + z as f32 + 0.5);
                        translucent.push((center.distance_squared(camera_position), face_vertices));
                    } else {
                        opaque.extend(face_vertices);
                    }
                }
                }
            }
        }
    }}
    translucent.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(std::cmp::Ordering::Equal));
    let translucent_vertices = translucent.into_iter().flat_map(|(_, face)| face).collect();
    let water_vertices = water.into_iter().flat_map(|(_, face)| face).collect();
    (opaque, translucent_vertices, water_vertices)
}

fn remap_water_uv_to_dedicated_texture(vertices: &mut [Vertex]) {
    // RenderBlocks rotates the top-face UVs around the center of one atlas
    // tile. At steep flow angles some corner UVs can cross an atlas boundary.
    // Resolve the source tile once for the whole face, then keep every sample
    // inside that water tile. Resolving per vertex accidentally sampled the
    // neighboring terrain tiles (the source of the unrelated texture on water).
    if vertices.is_empty() { return; }
    let inv_count = 1.0 / vertices.len() as f32;
    let center_u = vertices.iter().map(|v| v.uv[0]).sum::<f32>() * inv_count;
    // Water occupies atlas tiles 205/206 (row 12, columns 13/14). Vanilla's
    // flowing-face UV origin sits at the far edge of tile 206, so its centroid
    // can fall in column 15 even though the intended source remains tile 206.
    let tile_column = ((center_u * 16.0).floor() as i32).clamp(13, 14);
    let tile_row = 12;
    let strip = if tile_column >= 14 { 1.0 } else { 0.0 };
    for vertex in vertices {
        let local_u = (vertex.uv[0] * 16.0 - tile_column as f32).clamp(0.0, 1.0);
        let local_v = (vertex.uv[1] * 16.0 - tile_row as f32).clamp(0.0, 1.0);
        vertex.uv = [(strip + local_u) * 0.5, local_v];
    }
}

fn build_fluid_faces(world: &world_client::WorldClient, id: u8, metadata: u8, x: i32, y: i32, z: i32, brightness: f32) -> Vec<Vec<Vertex>> {
    let base_tile = if id <= 9 { 205_u32 } else { 237_u32 };
    let fluid_is_water = id <= 9;
    let same_fluid = |block_id: u8| if fluid_is_water { matches!(block_id, 8 | 9) } else { matches!(block_id, 10 | 11) };
    let neighbor_id = |dx: i32, dy: i32, dz: i32| world.block_at(x + dx, y + dy, z + dz);
    let mut result = Vec::with_capacity(6);
    // RenderBlocks uses Tessellator.setColorOpaque_F for fluid faces; water
    // opacity comes from TextureWaterFX's per-pixel alpha, not vertex alpha.
    let alpha = 1.0;
    let heights = [
        fluid_corner_height(world, x, y, z, fluid_is_water),
        fluid_corner_height(world, x, y, z + 1, fluid_is_water),
        fluid_corner_height(world, x + 1, y, z + 1, fluid_is_water),
        fluid_corner_height(world, x + 1, y, z, fluid_is_water),
    ];

    if !same_fluid(neighbor_id(0, 1, 0)) && neighbor_id(0, 1, 0) != 79 {
        let angle = fluid_flow_angle(world, x, y, z, metadata, fluid_is_water);
        let flowing = angle > -999.0;
        let tile = if flowing { base_tile + 1 } else { base_tile };
        let tile_x = (tile % 16) as f32 * 16.0;
        let tile_y = (tile / 16) as f32 * 16.0;
        let angle = if angle <= -999.0 { 0.0 } else { angle as f32 };
        let sin = angle.sin() * 8.0;
        let cos = angle.cos() * 8.0;
        let (center_u, center_v) = if !flowing {
            (tile_x + 8.0, tile_y + 8.0)
        } else {
            (tile_x + 16.0, tile_y + 16.0)
        };
        let corners = [
            ([x as f32, y as f32 + heights[0], z as f32], [(center_u - cos - sin) / 256.0, (center_v - cos + sin) / 256.0]),
            ([x as f32, y as f32 + heights[1], z as f32 + 1.0], [(center_u - cos + sin) / 256.0, (center_v + cos + sin) / 256.0]),
            ([x as f32 + 1.0, y as f32 + heights[2], z as f32 + 1.0], [(center_u + cos + sin) / 256.0, (center_v + cos - sin) / 256.0]),
            ([x as f32 + 1.0, y as f32 + heights[3], z as f32], [(center_u + cos - sin) / 256.0, (center_v - cos - sin) / 256.0]),
        ];
        result.push(fluid_quad(corners, [brightness, brightness, brightness, alpha]));
    }

    if !same_fluid(neighbor_id(0, -1, 0)) && !block_definition(neighbor_id(0, -1, 0)).opaque {
        let tile_x = base_tile % 16;
        let tile_y = base_tile / 16;
        let points = [
            ([x as f32, y as f32, z as f32], [tile_x as f32 / 16.0, (tile_y + 1) as f32 / 16.0]),
            ([x as f32, y as f32, z as f32 + 1.0], [(tile_x + 1) as f32 / 16.0, (tile_y + 1) as f32 / 16.0]),
            ([x as f32 + 1.0, y as f32, z as f32 + 1.0], [(tile_x + 1) as f32 / 16.0, tile_y as f32 / 16.0]),
            ([x as f32 + 1.0, y as f32, z as f32], [tile_x as f32 / 16.0, tile_y as f32 / 16.0]),
        ];
        result.push(fluid_quad(points, [brightness * 0.5, brightness * 0.5, brightness * 0.5, alpha]));
    }

    let sides = [
        ((0, 0, -1), [
            ([x as f32, y as f32 + heights[0], z as f32], heights[0]),
            ([x as f32 + 1.0, y as f32 + heights[3], z as f32], heights[3]),
            ([x as f32 + 1.0, y as f32, z as f32], 0.0),
            ([x as f32, y as f32, z as f32], 0.0),
        ], 0usize),
        ((0, 0, 1), [
            ([x as f32 + 1.0, y as f32 + heights[2], z as f32 + 1.0], heights[2]),
            ([x as f32, y as f32 + heights[1], z as f32 + 1.0], heights[1]),
            ([x as f32, y as f32, z as f32 + 1.0], 0.0),
            ([x as f32 + 1.0, y as f32, z as f32 + 1.0], 0.0),
        ], 1usize),
        ((-1, 0, 0), [
            ([x as f32, y as f32 + heights[1], z as f32 + 1.0], heights[1]),
            ([x as f32, y as f32 + heights[0], z as f32], heights[0]),
            ([x as f32, y as f32, z as f32], 0.0),
            ([x as f32, y as f32, z as f32 + 1.0], 0.0),
        ], 2usize),
        ((1, 0, 0), [
            ([x as f32 + 1.0, y as f32 + heights[3], z as f32], heights[3]),
            ([x as f32 + 1.0, y as f32 + heights[2], z as f32 + 1.0], heights[2]),
            ([x as f32 + 1.0, y as f32, z as f32 + 1.0], 0.0),
            ([x as f32 + 1.0, y as f32, z as f32], 0.0),
        ], 3usize),
    ];
    for ((dx, dy, dz), points, side) in sides {
        let neighbor = neighbor_id(dx, dy, dz);
        if same_fluid(neighbor) || neighbor == 79 || block_definition(neighbor).opaque { continue; }
        let tile = base_tile + 1;
        let u0 = (tile % 16) as f32 / 16.0;
        let u1 = (tile % 16) as f32 / 16.0 + 15.99 / 256.0;
        let v0 = (tile / 16) as f32 / 16.0;
        let bottom_v = (tile / 16) as f32 / 16.0 + 15.99 / 256.0;
        let shade = if side < 2 { 0.8 } else { 0.6 };
        let vertices = points.map(|(position, height)| {
            let v = v0 + (1.0 - height) * (16.0 / 256.0);
            (position, [u0, v])
        });
        let mut uv_points = vertices;
        uv_points[1].1[0] = u1;
        uv_points[2].1 = [u1, bottom_v];
        uv_points[3].1 = [u0, bottom_v];
        result.push(fluid_quad(uv_points, [brightness * shade, brightness * shade, brightness * shade, alpha]));
    }
    result
}

fn fluid_quad(points: [([f32; 3], [f32; 2]); 4], color: [f32; 4]) -> Vec<Vertex> {
    [0usize, 1, 2, 0, 2, 3].into_iter().map(|index| {
        Vertex { position: points[index].0, uv: points[index].1, color }
    }).collect()
}

fn fluid_corner_height(world: &world_client::WorldClient, x: i32, y: i32, z: i32, water: bool) -> f32 {
    let same_fluid = |id: u8| if water { matches!(id, 8 | 9) } else { matches!(id, 10 | 11) };
    let mut total = 0.0;
    let mut samples = 0.0;
    for (dx, dz) in [(0, 0), (-1, 0), (0, -1), (-1, -1)] {
        let sx = x + dx;
        let sz = z + dz;
        if same_fluid(world.block_at(sx, y + 1, sz)) { return 1.0; }
        let (id, mut metadata, _) = world.state_at(sx, y, sz);
        if same_fluid(id) {
            if metadata >= 8 { metadata = 0; }
            let air = (metadata as f32 + 1.0) / 9.0;
            if metadata == 0 {
                total += air * 10.0;
                samples += 10.0;
            }
            total += air;
            samples += 1.0;
        } else if !block_definition(id).solid {
            total += 1.0;
            samples += 1.0;
        }
    }
    if samples == 0.0 { 1.0 } else { 1.0 - total / samples }
}

fn fluid_flow_angle(world: &world_client::WorldClient, x: i32, y: i32, z: i32, metadata: u8, water: bool) -> f64 {
    let same_fluid = |id: u8| if water { matches!(id, 8 | 9) } else { matches!(id, 10 | 11) };
    let effective = |id: u8, metadata: u8| if same_fluid(id) { Some(if metadata >= 8 { 0_i32 } else { metadata as i32 }) } else { None };
    let current = effective(world.block_at(x, y, z), metadata).unwrap_or(0);
    let mut flow = DVec3::ZERO;
    for (dx, dz) in [(-1, 0), (0, -1), (1, 0), (0, 1)] {
        let nx = x + dx;
        let nz = z + dz;
        let (neighbor_id, neighbor_meta, _) = world.state_at(nx, y, nz);
        let difference = if let Some(decay) = effective(neighbor_id, neighbor_meta) {
            Some(decay - current)
        } else if !block_material_is_solid(neighbor_id) {
            effective(world.block_at(nx, y - 1, nz), world.state_at(nx, y - 1, nz).1).map(|below| below - (current - 8))
        } else { None };
        if let Some(difference) = difference {
            flow.x += (dx * difference) as f64;
            flow.z += (dz * difference) as f64;
        }
    }
    if metadata >= 8 {
        let down_force = [
            (0, y, -1, 2), (0, y, 1, 3), (-1, y, 0, 4), (1, y, 0, 5),
            (0, y + 1, -1, 2), (0, y + 1, 1, 3), (-1, y + 1, 0, 4), (1, y + 1, 0, 5),
        ].into_iter().any(|(dx, ny, dz, side)| {
            fluid_is_block_solid(world, x + dx, ny, z + dz, side, water)
        });
        if down_force {
            let length = flow.length();
            if length >= 1.0e-4 { flow /= length; } else { flow = DVec3::ZERO; }
            flow.y -= 6.0;
        }
    }
    let length = flow.length();
    if length < 1.0e-4 { flow = DVec3::ZERO; } else { flow /= length; }
    if flow.x == 0.0 && flow.z == 0.0 { -1000.0 } else { flow.z.atan2(flow.x) - std::f64::consts::FRAC_PI_2 }
}

/// Approximation of Material.getIsSolid from the Beta block material mapping.
/// This is distinct from isOpaqueCube and from the block collision boxes.
fn block_material_is_solid(id: u8) -> bool {
    block_definition(id).solid
}

/// Beta BlockFluid.getIsBlockSolid: fluid and ice are never solid to the
/// falling-fluid check; the upper face is otherwise considered solid.
fn fluid_is_block_solid(world: &world_client::WorldClient, x: i32, y: i32, z: i32, side: i32, water: bool) -> bool {
    let id = world.block_at(x, y, z);
    let same_fluid = if water { matches!(id, 8 | 9) } else { matches!(id, 10 | 11) };
    if same_fluid || id == 79 { false }
    else if side == 1 { true }
    else { block_material_is_solid(id) }
}

fn build_rail_mesh(id: u8, metadata: u8, x: f32, y: f32, z: f32, brightness: f32) -> Vec<Vertex> {
    let meta = metadata & 7;
    let (west, east, north, south) = match meta {
        2 => (1.0 / 16.0, 17.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0),
        3 => (17.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0),
        4 => (1.0 / 16.0, 15.0 / 16.0, 17.0 / 16.0, 1.0 / 16.0),
        5 => (1.0 / 16.0, 15.0 / 16.0, 1.0 / 16.0, 17.0 / 16.0),
        _ => (1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0),
    };
    let corners = [
        [x, y + north, z + 1.0], [x + 1.0, y + south, z + 1.0],
        [x + 1.0, y + east, z], [x, y + west, z],
    ];
    let tile = oldmine_common::world::block_face_tile(id, metadata, 3) as u32;
    let tx = (tile % 16) as f32 / 16.0;
    let ty = (tile / 16) as f32 / 16.0;
    let uv = [[tx, ty], [tx + 1.0 / 16.0, ty], [tx + 1.0 / 16.0, ty + 1.0 / 16.0], [tx, ty + 1.0 / 16.0]];
    [0usize, 1, 2, 0, 2, 3].into_iter().map(|i| Vertex { position: corners[i], uv: uv[i], color: [brightness, brightness, brightness, 1.0] }).collect()
}

fn build_chunk_mesh(world: &world_client::WorldClient, _x: i32, _z: i32, camera_position: Vec3) -> ChunkMesh {
    let (opaque, translucent, water) = build_world_mesh(world, camera_position);
    ChunkMesh { opaque, translucent: vec![(0.0, translucent)], water: vec![(0.0, water)] }
}

fn build_entity_mesh(entities: &std::collections::HashMap<i32, state::RemoteEntity>) -> Vec<Vertex> {
    let mut vertices = Vec::new();
    let faces = [
        [[0.,0.,0.],[0.,0.,1.],[0.,1.,1.],[0.,1.,0.]], [[1.,0.,1.],[1.,0.,0.],[1.,1.,0.],[1.,1.,1.]],
        [[0.,0.,1.],[1.,0.,1.],[1.,0.,0.],[0.,0.,0.]], [[0.,1.,0.],[1.,1.,0.],[1.,1.,1.],[0.,1.,1.]],
        [[1.,0.,0.],[0.,0.,0.],[0.,1.,0.],[1.,1.,0.]], [[0.,0.,1.],[1.,0.,1.],[1.,1.,1.],[0.,1.,1.]],
    ];
    let triangles = [0, 1, 2, 0, 2, 3];
    for entity in entities.values() {
        let (width, height, color) = match entity.kind {
            1 => (0.6, 1.8, [0.25, 0.45, 0.85, 1.0]),
            2 => (0.25, 0.25, [0.82, 0.72, 0.42, 1.0]),
            _ => (0.45, 0.45, [0.75, 0.35, 0.25, 1.0]),
        };
        let min = entity.position - Vec3::new(width/2.0, 0.0, width/2.0);
        let size = [width, height, width];
        for face in faces {
            for index in triangles {
                let point = face[index];
                vertices.push(Vertex { position: [min.x + point[0]*size[0], min.y + point[1]*size[1], min.z + point[2]*size[2]], uv: [0.0, 0.0], color });
            }
        }
    }
    vertices
}

fn build_celestial_mesh(camera: Vec3, world_time: f32) -> (Vec<Vertex>, Vec<Vertex>) {
    let angle = celestial_angle(world_time) * std::f32::consts::TAU;
    let rotate = |p: [f32; 3]| -> [f32; 3] {
        [camera.x + p[0], camera.y + p[1] * angle.cos() - p[2] * angle.sin(), camera.z + p[1] * angle.sin() + p[2] * angle.cos()]
    };
    let quad = |points: [[f32; 3]; 4]| -> Vec<Vertex> {
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        [0usize, 1, 2, 0, 2, 3].into_iter().map(|i| Vertex { position: rotate(points[i]), uv: uv[i], color: [1.0, 1.0, 1.0, 1.0] }).collect()
    };
    let sun = quad([[-30.0, 100.0, -30.0], [30.0, 100.0, -30.0], [30.0, 100.0, 30.0], [-30.0, 100.0, 30.0]]);
    let moon = quad([[-20.0, -100.0, 20.0], [20.0, -100.0, 20.0], [20.0, -100.0, -20.0], [-20.0, -100.0, -20.0]]);
    (sun, moon)
}

fn load_png_texture(device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8], label: &str) -> Result<(wgpu::TextureView, wgpu::Sampler), Box<dyn std::error::Error>> {
    let rgba = image::load_from_memory(bytes)?.to_rgba8();
    let size = wgpu::Extent3d { width: rgba.width(), height: rgba.height(), depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * rgba.width()), rows_per_image: Some(rgba.height()) }, size);
    let view = texture.create_view(&Default::default());
    let address_mode = if label == "clouds.png" { wgpu::AddressMode::Repeat } else { wgpu::AddressMode::ClampToEdge };
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, address_mode_u: address_mode, address_mode_v: address_mode, ..Default::default() });
    Ok((view, sampler))
}

fn load_water_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    initial_animation: &[u8; 16 * 16 * 4],
) -> Result<(wgpu::Texture, wgpu::TextureView, wgpu::Sampler), Box<dyn std::error::Error>> {
    let mut tiles = image::RgbaImage::new(32, 16);
    let flowing = image::load_from_memory(include_bytes!("../assets/terrain/water-flowing.png"))?.to_rgba8();
    let side = image::load_from_memory(include_bytes!("../assets/terrain/water-side.png"))?.to_rgba8();
    image::imageops::replace(&mut tiles, &flowing, 0, 0);
    image::imageops::replace(&mut tiles, &side, 16, 0);
    let size = wgpu::Extent3d { width: 32, height: 16, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dedicated Beta water tiles 205 and 206"), size,
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &tiles,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(32 * 4), rows_per_image: Some(16) },
        size,
    );
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        initial_animation,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(16 * 4), rows_per_image: Some(16) },
        wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
    );
    let view = texture.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("nearest clamp water sampler"),
        mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest,
        address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    Ok((texture, view, sampler))
}

fn load_terrain_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(wgpu::Texture, wgpu::TextureView, wgpu::Sampler), Box<dyn std::error::Error>> {
    let rgba = image::load_from_memory(include_bytes!("../assets/terrain/terrain.png"))?.to_rgba8();
    let size = wgpu::Extent3d { width: rgba.width(), height: rgba.height(), depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor { label: Some("terrain.png"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * rgba.width()), rows_per_image: Some(rgba.height()) }, size);
    let view = texture.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, ..Default::default() });
    Ok((texture, view, sampler))
}

/// Mirrors RenderEngine.updateDynamicTextures for LavaFX (tiles 237/238).
/// The flowing animation declares tileSize=2 and is copied into a 2x2 tile area.
fn upload_lava_fx(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    pixels: &[u8; 16 * 16 * 4],
    icon_index: u32,
    tile_size: u32,
) {
    for tile_y in 0..tile_size {
        for tile_x in 0..tile_size {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: (icon_index % 16) * 16 + tile_x * 16,
                        y: (icon_index / 16) * 16 + tile_y * 16,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(16 * 4),
                    rows_per_image: Some(16),
                },
                wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            );
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    EventLoop::new()?.run_app(&mut ClientApp::default())?;
    Ok(())
}
