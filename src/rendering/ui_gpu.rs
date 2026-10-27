//! One unlit, alpha-blended draw for a cached UI display list and 4 MiB atlas.

use std::{io::BufReader, path::Path};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::ui::{DESIGN_SIZE, DrawCommand, Rect, UiFrame, UiImage};

const ATLAS_SIZE: u32 = 1024;
const GLYPH_Y: u32 = 896;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Quad {
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
}

impl Quad {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

#[derive(Clone, Copy)]
struct Sprite {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    transform: wgpu::Buffer,
    instances: wgpu::Buffer,
    capacity: usize,
    quads: Vec<Quad>,
    cached_revision: Option<u64>,
    viewport: [u32; 2],
    splash: Option<Sprite>,
    menu: Option<Sprite>,
}

impl UiRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        assets: &Path,
    ) -> Self {
        let (pixels, splash, menu) = build_atlas(assets);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("UI atlas / images + bitmap font"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_SIZE * 4),
                rows_per_image: Some(ATLAS_SIZE),
            },
            wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
        );
        // pixels is released here after upload; no full-resolution background is retained.
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("UI atlas sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let transform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("UI logical canvas transform"),
            contents: bytemuck::cast_slice(&[0_f32; 4]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("UI atlas layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UI atlas bindings"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: transform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Unlit UI overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("UI pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Batched unlit UI"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Quad::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
        });
        Self {
            pipeline,
            bind_group,
            transform,
            instances: instance_buffer(device, 2048),
            capacity: 2048,
            quads: Vec::with_capacity(2048),
            cached_revision: None,
            viewport: [0, 0],
            splash,
            menu,
        }
    }

    /// Geometry changes only when the retained UI revision changes. Resize updates one vec4.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: [u32; 2],
        frame: &UiFrame,
    ) {
        if viewport.contains(&0) {
            return;
        }
        if self.viewport != viewport {
            let scale =
                (viewport[0] as f32 / DESIGN_SIZE[0]).min(viewport[1] as f32 / DESIGN_SIZE[1]);
            let offset = [
                (viewport[0] as f32 - DESIGN_SIZE[0] * scale) * 0.5,
                (viewport[1] as f32 - DESIGN_SIZE[1] * scale) * 0.5,
            ];
            let transform = [
                2. * scale / viewport[0] as f32,
                -2. * scale / viewport[1] as f32,
                2. * offset[0] / viewport[0] as f32 - 1.,
                1. - 2. * offset[1] / viewport[1] as f32,
            ];
            queue.write_buffer(&self.transform, 0, bytemuck::cast_slice(&transform));
            self.viewport = viewport;
        }
        if self.cached_revision == Some(frame.revision) {
            return;
        }
        self.quads.clear();
        for command in &frame.commands {
            match command {
                DrawCommand::Rect(rect, color) => self.push(
                    *rect,
                    [201.5 / 1024., 901.5 / 1024., 201.5 / 1024., 901.5 / 1024.],
                    *color,
                ),
                DrawCommand::Text {
                    position,
                    text,
                    scale,
                    color,
                } => {
                    for (i, c) in text.chars().enumerate() {
                        if c == ' ' {
                            continue;
                        }
                        let code = if c.is_ascii() {
                            c.to_ascii_uppercase() as u32
                        } else {
                            '?' as u32
                        };
                        let x = (code % 16) * 8;
                        let y = GLYPH_Y + (code / 16) * 10;
                        self.push(
                            Rect::new(
                                position[0] + i as f32 * 8. * scale,
                                position[1],
                                8. * scale,
                                8. * scale,
                            ),
                            [
                                x as f32 / 1024.,
                                y as f32 / 1024.,
                                (x + 8) as f32 / 1024.,
                                (y + 8) as f32 / 1024.,
                            ],
                            *color,
                        );
                    }
                }
                DrawCommand::Image(image, rect, tint) => {
                    let sprite = match image {
                        UiImage::Splash => self.splash,
                        UiImage::Menu => self.menu,
                    };
                    if let Some(sprite) = sprite {
                        let source_aspect = sprite.width as f32 / sprite.height as f32;
                        let target_aspect = rect.w / rect.h;
                        let (u, v) = if source_aspect > target_aspect {
                            (target_aspect / source_aspect, 1.)
                        } else {
                            (1., source_aspect / target_aspect)
                        };
                        let x = sprite.x as f32 + sprite.width as f32 * (1. - u) * 0.5;
                        let y = sprite.y as f32 + sprite.height as f32 * (1. - v) * 0.5;
                        self.push(
                            *rect,
                            [
                                x / 1024.,
                                y / 1024.,
                                (x + sprite.width as f32 * u) / 1024.,
                                (y + sprite.height as f32 * v) / 1024.,
                            ],
                            *tint,
                        );
                    } else {
                        self.push(
                            *rect,
                            [201.5 / 1024., 901.5 / 1024., 201.5 / 1024., 901.5 / 1024.],
                            [0.01, 0.025, 0.045, 1.],
                        );
                    }
                }
            }
        }
        if self.quads.len() > self.capacity {
            self.capacity = self.quads.len().next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        if !self.quads.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.quads));
        }
        self.cached_revision = Some(frame.revision);
    }

    fn push(&mut self, rect: Rect, uv: [f32; 4], color: [f32; 4]) {
        self.quads.push(Quad {
            rect: [rect.x, rect.y, rect.w, rect.h],
            uv,
            color,
        });
    }

    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        if self.quads.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..self.quads.len() as u32);
    }

    pub fn quad_count(&self) -> usize {
        self.quads.len()
    }
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Cached UI quad instances"),
        size: (capacity * std::mem::size_of::<Quad>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn build_atlas(assets: &Path) -> (Vec<u8>, Option<Sprite>, Option<Sprite>) {
    let mut atlas = vec![0_u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
    let splash = load_image(&mut atlas, &assets.join("splash.png"), 0);
    let menu = load_image(&mut atlas, &assets.join("Start_background.png"), 440);
    for code in 0_u32..128 {
        let glyph = glyph(char::from_u32(code).unwrap());
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let x = (code % 16) * 8 + column + 1;
                    let y = GLYPH_Y + (code / 16) * 10 + row as u32;
                    let offset = ((y * ATLAS_SIZE + x) * 4) as usize;
                    atlas[offset..offset + 4].fill(255);
                }
            }
        }
    }
    for y in 900..904 {
        for x in 200..204 {
            let offset = ((y * ATLAS_SIZE + x) * 4) as usize;
            atlas[offset..offset + 4].fill(255);
        }
    }
    (atlas, splash, menu)
}

fn load_image(atlas: &mut [u8], path: &Path, destination_y: u32) -> Option<Sprite> {
    match decode_image(atlas, path, destination_y) {
        Ok(sprite) => Some(sprite),
        Err(error) => {
            log::warn!("UI image {}: {error}; using placeholder", path.display());
            None
        }
    }
}

fn decode_image(
    atlas: &mut [u8],
    path: &Path,
    destination_y: u32,
) -> Result<Sprite, Box<dyn std::error::Error>> {
    let mut decoder = png::Decoder::new(BufReader::new(std::fs::File::open(path)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: 32 * 1024 * 1024,
    });
    let mut reader = decoder.read_info()?;
    if reader.output_buffer_size() > 32 * 1024 * 1024 {
        return Err("PNG exceeds the UI decode budget".into());
    }
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer)?;
    if info.width == 0 || info.height == 0 {
        return Err("empty PNG".into());
    }
    let scale = (1024. / info.width as f32)
        .min(432. / info.height as f32)
        .min(1.);
    let width = (info.width as f32 * scale).max(1.) as u32;
    let height = (info.height as f32 * scale).max(1.) as u32;
    let channels = info.color_type.samples();
    for y in 0..height {
        for x in 0..width {
            let sx = (x as u64 * info.width as u64 / width as u64) as usize;
            let sy = (y as u64 * info.height as u64 / height as u64) as usize;
            let source = (sy * info.width as usize + sx) * channels;
            let color = match info.color_type {
                png::ColorType::Rgba => [
                    buffer[source],
                    buffer[source + 1],
                    buffer[source + 2],
                    buffer[source + 3],
                ],
                png::ColorType::Rgb => {
                    [buffer[source], buffer[source + 1], buffer[source + 2], 255]
                }
                png::ColorType::Grayscale => [buffer[source], buffer[source], buffer[source], 255],
                png::ColorType::GrayscaleAlpha => [
                    buffer[source],
                    buffer[source],
                    buffer[source],
                    buffer[source + 1],
                ],
                _ => return Err("unsupported PNG output format".into()),
            };
            let offset = (((destination_y + y) * ATLAS_SIZE + x) * 4) as usize;
            atlas[offset..offset + 4].copy_from_slice(&color);
        }
    }
    Ok(Sprite {
        x: 0,
        y: destination_y,
        width,
        height,
    })
}

/// Original 5x7 placeholder glyphs; no external font file or font runtime required.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 25, 21, 19, 19, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '.' => [0, 0, 0, 0, 0, 6, 6],
        ',' => [0, 0, 0, 0, 0, 4, 8],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '%' => [25, 25, 2, 4, 8, 19, 19],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        '|' => [4, 4, 4, 4, 4, 4, 4],
        '=' => [0, 0, 31, 0, 31, 0, 0],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}
