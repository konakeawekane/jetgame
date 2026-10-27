//! Explicit GPU check: cargo test --test forward_gpu -- --ignored --nocapture

use std::{fs::File, io::BufWriter, path::Path, sync::mpsc};

use glam::{Mat4, Vec3};
use jetgame::rendering::{
    Camera, RenderObject, Scene,
    context::DEPTH_FORMAT,
    forward::ForwardPipeline,
    mesh::{MeshAssets, MeshData, Vertex},
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

fn read_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut ForwardPipeline,
    assets: &MeshAssets,
    scene: &Scene,
) -> Vec<u8> {
    let size = wgpu::Extent3d {
        width: WIDTH,
        height: HEIGHT,
        depth_or_array_layers: 1,
    };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Forward test color"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Forward test depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Forward test readback"),
        size: (WIDTH * HEIGHT * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let view = color.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    pipeline
        .prepare(device, queue, [WIDTH, HEIGHT], assets, scene)
        .unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Forward GPU verification"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.025,
                        g: 0.035,
                        b: 0.055,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pipeline.draw(&mut pass, assets, scene);
    }
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        size,
    );
    queue.submit(Some(encoder.finish()));
    let (sender, receiver) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device.poll(wgpu::Maintain::Wait);
    receiver.recv().unwrap().unwrap();
    let pixels = readback.slice(..).get_mapped_range().to_vec();
    readback.unmap();
    pixels
}

#[test]
#[ignore = "requires a hardware graphics adapter"]
fn supplied_meshes_render_and_depth_is_order_independent() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        },
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("hardware graphics adapter");
    println!("GPU test adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("Forward test device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut pipeline = ForwardPipeline::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut assets = MeshAssets::default();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let previews = root.join("target/render-previews");
    std::fs::create_dir_all(&previews).unwrap();

    for file in [
        "f-16_fighter_jet.glb",
        "world_01.glb",
        "bullet.glb",
        "rocket.glb",
    ] {
        let path = root.join("assets").join(file);
        let handle = assets.load(&device, &path).unwrap();
        assert_eq!(
            assets.load(&device, &path).unwrap(),
            handle,
            "cached GPU handle"
        );
        let mut scene = Scene::default();
        scene.objects.push(RenderObject::new(handle));
        scene.camera.frame_bounds(
            assets.get(handle).unwrap().bounds,
            WIDTH as f32 / HEIGHT as f32,
        );
        let pixels = read_frame(&device, &queue, &mut pipeline, &assets, &scene);
        let background = &pixels[..4];
        let shaded: Vec<_> = pixels
            .chunks_exact(4)
            .filter(|pixel| *pixel != background)
            .collect();
        assert!(shaded.len() > 100, "{file}: expected visible triangles");
        let min = shaded.iter().map(|p| p[0]).min().unwrap();
        let max = shaded.iter().map(|p| p[0]).max().unwrap();
        assert!(max - min > 5, "{file}: expected varying surface lighting");
        let output = previews.join(format!(
            "{}.png",
            Path::new(file).file_stem().unwrap().to_string_lossy()
        ));
        let mut png = png::Encoder::new(
            BufWriter::new(File::create(&output).unwrap()),
            WIDTH,
            HEIGHT,
        );
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
        println!("{file}: {} lit pixels; {}", shaded.len(), output.display());
    }
    assert_eq!(assets.len(), 4);

    // Differently lit overlapping surfaces must produce the same pixels in either order.
    let triangle = |normal| MeshData {
        vertices: vec![
            Vertex {
                position: [-1., -1., 0.],
                normal,
            },
            Vertex {
                position: [1., -1., 0.],
                normal,
            },
            Vertex {
                position: [0., 1., 0.],
                normal,
            },
        ],
        indices: vec![0, 1, 2],
    };
    let near_mesh = assets
        .upload(&device, "Near triangle", &triangle([0., 0., 1.]))
        .unwrap();
    let far_mesh = assets
        .upload(&device, "Far triangle", &triangle([0., 0., -1.]))
        .unwrap();
    let near = RenderObject {
        mesh: near_mesh,
        transform: Mat4::from_translation(Vec3::Z * 0.5),
    };
    let far = RenderObject::new(far_mesh);
    let mut scene = Scene {
        camera: Camera {
            eye: Vec3::Z * 4.,
            target: Vec3::ZERO,
            ..Default::default()
        },
        ..Default::default()
    };
    scene.light.direction = Vec3::Z;
    scene.objects = vec![near, far];
    let near_first = read_frame(&device, &queue, &mut pipeline, &assets, &scene);
    scene.objects.reverse();
    assert_eq!(
        near_first,
        read_frame(&device, &queue, &mut pipeline, &assets, &scene)
    );
    scene.objects = vec![far];
    assert_ne!(
        near_first,
        read_frame(&device, &queue, &mut pipeline, &assets, &scene)
    );
    assert!(pollster::block_on(device.pop_error_scope()).is_none());
}
