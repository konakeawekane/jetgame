//! Shared mesh resources and a single forward path for rigid 3D objects.

pub mod context;
pub mod forward;
pub mod frame;
pub mod mesh;
pub mod shadow_bake;

use std::{error::Error, path::Path, sync::Arc};
use winit::{dpi::PhysicalSize, window::Window};

pub use context::GpuContext;
pub use forward::{Camera, DirectionalLight, RenderObject, Scene, SurfaceMaterial};
use forward::{ForwardPipeline, SceneError};
pub use frame::FrameContext;
use mesh::MeshAssets;
pub use mesh::{MeshData, MeshError, MeshHandle};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Surface(#[from] wgpu::SurfaceError),
    #[error(transparent)]
    Scene(#[from] SceneError),
}

pub struct Renderer {
    gpu: GpuContext,
    meshes: MeshAssets,
    forward: ForwardPipeline,
    pub scene: Scene,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, Box<dyn Error>> {
        let gpu = GpuContext::new(window).await?;
        let forward = ForwardPipeline::new(&gpu.device, gpu.color_format());
        Ok(Self {
            gpu,
            meshes: MeshAssets::default(),
            forward,
            scene: Scene::default(),
        })
    }

    /// Resolve a file address into a reusable GPU resource handle.
    pub fn load_mesh(&mut self, path: impl AsRef<Path>) -> Result<MeshHandle, MeshError> {
        self.meshes.load(&self.gpu.device, path)
    }

    /// Format-independent entry point for future custom mesh packs.
    pub fn upload_mesh(&mut self, label: &str, data: &MeshData) -> Result<MeshHandle, MeshError> {
        self.meshes.upload(&self.gpu.device, label, data)
    }

    pub fn mesh_assets(&self) -> &MeshAssets {
        &self.meshes
    }

    pub fn frame_mesh(&mut self, mesh: MeshHandle) -> Result<(), SceneError> {
        let bounds = self
            .meshes
            .get(mesh)
            .ok_or_else(|| SceneError("unknown mesh handle".into()))?
            .bounds;
        let size = self.gpu.size();
        self.scene
            .camera
            .frame_bounds(bounds, size.width.max(1) as f32 / size.height.max(1) as f32);
        Ok(())
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
    }

    pub fn render(&mut self, _frame: &FrameContext) -> Result<bool, RenderError> {
        let Some(output) = self.gpu.acquire_frame()? else {
            return Ok(false);
        };
        let size = self.gpu.size();
        self.forward.prepare(
            &self.gpu.device,
            &self.gpu.queue,
            [size.width, size.height],
            &self.meshes,
            &self.scene,
        )?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Forward frame encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Universal forward scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.025,
                            g: 0.035,
                            b: 0.055,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.gpu.depth_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.forward.draw(&mut pass, &self.meshes, &self.scene);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(true)
    }
}
