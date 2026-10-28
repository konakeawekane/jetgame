//! Shared mesh resources and a single forward path for rigid 3D objects.

pub mod context;
pub mod forward;
pub mod frame;
pub mod mesh;
pub mod shadow_bake;
pub mod ui_gpu;

use std::{error::Error, path::Path, sync::Arc};
use winit::{dpi::PhysicalSize, window::Window};

use crate::ui::UiFrame;
pub use context::GpuContext;
pub use forward::{Camera, DirectionalLight, RenderObject, Scene, SurfaceMaterial};
use forward::{ForwardPipeline, SceneError};
pub use frame::FrameContext;
use mesh::MeshAssets;
pub use mesh::{MeshData, MeshError, MeshHandle};
use ui_gpu::UiRenderer;

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
    overlay: UiRenderer,
    pub scene: Scene,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, Box<dyn Error>> {
        Self::new_with_assets(window, Path::new("assets")).await
    }
    pub async fn new_with_assets(
        window: Arc<Window>,
        asset_root: &Path,
    ) -> Result<Self, Box<dyn Error>> {
        let gpu = GpuContext::new(window).await?;
        let forward = ForwardPipeline::new(&gpu.device, gpu.color_format());
        let overlay = UiRenderer::new(&gpu.device, &gpu.queue, gpu.color_format(), asset_root);
        Ok(Self {
            gpu,
            meshes: MeshAssets::default(),
            forward,
            overlay,
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

    pub fn mesh_resources(&mut self) -> (&wgpu::Device, &mut MeshAssets) {
        (&self.gpu.device, &mut self.meshes)
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

    pub fn set_vsync(&mut self, enabled: bool) {
        self.gpu.set_vsync(enabled);
    }

    pub fn render(&mut self, frame: &FrameContext) -> Result<bool, RenderError> {
        self.render_frame(frame, None)
    }

    pub fn render_with_ui(
        &mut self,
        frame: &FrameContext,
        ui: &UiFrame,
    ) -> Result<bool, RenderError> {
        self.render_frame(frame, Some(ui))
    }

    fn render_frame(
        &mut self,
        _frame: &FrameContext,
        ui: Option<&UiFrame>,
    ) -> Result<bool, RenderError> {
        let Some(output) = self.gpu.acquire_frame()? else {
            return Ok(false);
        };
        let size = self.gpu.size();
        let show_world = ui.is_none_or(|ui| ui.show_world);
        let world_viewport = ui.and_then(|ui| ui.world_viewport([size.width, size.height]));
        if show_world {
            self.forward.prepare(
                &self.gpu.device,
                &self.gpu.queue,
                world_viewport.map_or([size.width, size.height], |r| {
                    [r[2].max(1.) as u32, r[3].max(1.) as u32]
                }),
                &self.meshes,
                &self.scene,
            )?;
        }
        if let Some(ui) = ui {
            self.overlay.prepare(
                &self.gpu.device,
                &self.gpu.queue,
                [size.width, size.height],
                ui,
            );
        }
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
                            r: self.scene.background[0],
                            g: self.scene.background[1],
                            b: self.scene.background[2],
                            a: self.scene.background[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: show_world.then(|| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view: self.gpu.depth_view(),
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if show_world {
                if let Some([x, y, width, height]) = world_viewport {
                    pass.set_viewport(x, y, width, height, 0., 1.);
                }
                self.forward.draw(&mut pass, &self.meshes, &self.scene);
            }
        }
        if ui.is_some() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Unlit user interface"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.overlay.draw(&mut pass);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(true)
    }
}
