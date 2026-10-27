use std::{error::Error, path::PathBuf, sync::Arc, time::Instant};

use winit::{
    dpi::PhysicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use crate::rendering::{FrameContext, RenderError, RenderObject, Renderer};

pub struct RunOptions {
    pub smoke_test: bool,
    pub mesh: PathBuf,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            smoke_test: false,
            mesh: "assets/f-16_fighter_jet.glb".into(),
        }
    }
}

/// Start the window loop. Smoke testing exits after three presented frames.
pub fn run(options: RunOptions) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("JetGame")
            // Development window size; the final 2K benchmark size is still undecided.
            .with_inner_size(PhysicalSize::new(1280, 720))
            .build(&event_loop)?,
    );
    let mut renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))?;
    let mesh = renderer.load_mesh(&options.mesh)?;
    renderer.scene.objects.push(RenderObject::new(mesh));
    renderer.frame_mesh(mesh)?;
    log::info!(
        "Loaded {}: {} GPU mesh bytes",
        options.mesh.display(),
        renderer.mesh_assets().buffer_bytes()
    );
    let start = Instant::now();
    let mut previous_frame = start;
    let mut frame_index = 0;
    let mut fatal_error = None;

    event_loop.run(|event, target| {
        target.set_control_flow(ControlFlow::Wait);
        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(size) => renderer.resize(size),
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    let frame = FrameContext {
                        frame_index,
                        delta_time: now.duration_since(previous_frame),
                        elapsed_time: now.duration_since(start),
                    };
                    previous_frame = now;

                    match renderer.render(&frame) {
                        Ok(presented) => {
                            if presented {
                                frame_index += 1;
                                if options.smoke_test && frame_index >= 3 {
                                    println!("Smoke test presented {frame_index} frames");
                                    target.exit();
                                }
                            }
                        }
                        Err(RenderError::Surface(
                            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated,
                        )) => {
                            renderer.resize(window.inner_size());
                        }
                        Err(RenderError::Surface(wgpu::SurfaceError::Timeout)) => {
                            log::warn!("Surface frame timed out")
                        }
                        Err(error) => {
                            fatal_error = Some(error.to_string());
                            target.exit();
                        }
                    }
                }
                _ => {}
            },
            Event::AboutToWait => {
                let size = window.inner_size();
                if size.width > 0 && size.height > 0 {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    })?;

    if let Some(error) = fatal_error {
        return Err(error.into());
    }
    if options.smoke_test && frame_index < 3 {
        return Err("Smoke test ended before three frames were presented".into());
    }
    Ok(())
}
