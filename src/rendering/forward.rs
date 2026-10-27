//! One forward path for every rigid mesh, with one shared Blinn-Phong material.

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::{
    context::DEPTH_FORMAT,
    mesh::{Bounds, MeshAssets, MeshHandle, Vertex},
};

#[derive(Debug, thiserror::Error)]
#[error("invalid render scene: {0}")]
pub struct SceneError(pub String);

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub vertical_fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: Vec3::new(3., 2., 4.),
            target: Vec3::ZERO,
            up: Vec3::Y,
            vertical_fov: 45_f32.to_radians(),
            near: 0.1,
            far: 1000.,
        }
    }
}

impl Camera {
    /// Frame a mesh in its loaded coordinates without changing its physical scale.
    pub fn frame_bounds(&mut self, bounds: Bounds, aspect: f32) {
        let radius = bounds.radius().max(0.001);
        let half_angle = ((self.vertical_fov * 0.5).tan() * aspect.clamp(0.01, 1.0)).atan();
        let distance = radius / half_angle.sin() * 1.15;
        self.target = bounds.center();
        self.eye = self.target + Vec3::new(1.0, 0.65, 1.4).normalize() * distance;
        self.near = (distance - radius * 1.5).max(radius * 0.001);
        self.far = distance + radius * 4.0;
    }

    fn view_projection(self, aspect: f32) -> Result<Mat4, SceneError> {
        if !self.eye.is_finite()
            || !self.target.is_finite()
            || !self.up.is_finite()
            || !self.vertical_fov.is_finite()
            || self.vertical_fov <= 0.0
            || self.vertical_fov >= std::f32::consts::PI
            || !self.near.is_finite()
            || !self.far.is_finite()
            || self.near <= 0.0
            || self.far <= self.near
            || (self.target - self.eye).cross(self.up).length_squared() < 1e-12
        {
            return Err(SceneError(
                "camera needs a valid perspective and view direction".into(),
            ));
        }
        // glam's RH projection uses the 0..1 depth range expected by wgpu.
        Ok(
            Mat4::perspective_rh(self.vertical_fov, aspect, self.near, self.far)
                * Mat4::look_at_rh(self.eye, self.target, self.up.normalize()),
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SurfaceMaterial {
    /// Linear RGB, shared by all objects for now.
    pub color: [f32; 3],
    pub ambient: f32,
    pub specular: f32,
    pub shininess: f32,
}

impl Default for SurfaceMaterial {
    fn default() -> Self {
        Self {
            color: [0.55, 0.60, 0.66],
            ambient: 0.15,
            specular: 0.35,
            shininess: 48.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    /// World-space direction from the surface toward the light.
    pub direction: Vec3,
    pub color: [f32; 3],
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: Vec3::new(0.4, 0.8, 0.6),
            color: [1.; 3],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderObject {
    pub mesh: MeshHandle,
    pub transform: Mat4,
}

impl RenderObject {
    pub fn new(mesh: MeshHandle) -> Self {
        Self {
            mesh,
            transform: Mat4::IDENTITY,
        }
    }
}

#[derive(Default)]
pub struct Scene {
    pub objects: Vec<RenderObject>,
    pub camera: Camera,
    pub material: SurfaceMaterial,
    pub light: DirectionalLight,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SceneUniform {
    view_projection: [[f32; 4]; 4],
    camera_position: [f32; 4],
    light_direction: [f32; 4],
    light_color: [f32; 4],
    color_ambient: [f32; 4],
    specular_shininess: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ObjectUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 3],
}

impl ObjectUniform {
    fn from_transform(transform: Mat4) -> Result<Self, SceneError> {
        let determinant = transform.determinant();
        if !transform.is_finite()
            || !determinant.is_finite()
            || determinant <= 0.0
            || transform.x_axis.w != 0.0
            || transform.y_axis.w != 0.0
            || transform.z_axis.w != 0.0
            || transform.w_axis.w != 1.0
        {
            return Err(SceneError(
                "object transforms must be finite, affine, nonsingular, and preserve handedness"
                    .into(),
            ));
        }
        let normal = Mat3::from_mat4(transform).inverse().transpose();
        if !normal.is_finite() {
            return Err(SceneError("invalid normal transform".into()));
        }
        Ok(Self {
            model: transform.to_cols_array_2d(),
            normal: [
                normal.x_axis.extend(0.).to_array(),
                normal.y_axis.extend(0.).to_array(),
                normal.z_axis.extend(0.).to_array(),
            ],
        })
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
            2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
            6 => Float32x4, 7 => Float32x4, 8 => Float32x4
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

pub struct ForwardPipeline {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    objects: wgpu::Buffer,
    object_capacity: usize,
    staging: Vec<ObjectUniform>,
}

impl ForwardPipeline {
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scene and universal surface material"),
            contents: bytemuck::bytes_of(&SceneUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Forward scene layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<SceneUniform>() as u64
                    ),
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Forward scene"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Universal forward layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Universal Blinn-Phong"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blinn_phong.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Universal forward pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout(), ObjectUniform::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
        });
        Self {
            pipeline,
            uniform,
            bind_group,
            objects: object_buffer(device, 1),
            object_capacity: 1,
            staging: Vec::new(),
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: [u32; 2],
        meshes: &MeshAssets,
        scene: &Scene,
    ) -> Result<(), SceneError> {
        if viewport.contains(&0) {
            return Err(SceneError("empty viewport".into()));
        }
        let view_projection = scene
            .camera
            .view_projection(viewport[0] as f32 / viewport[1] as f32)?;
        let material = scene.material;
        if !scene.light.direction.is_finite()
            || scene.light.direction.length_squared() < 1e-12
            || material
                .color
                .iter()
                .chain(scene.light.color.iter())
                .any(|v| !v.is_finite() || *v < 0.0)
            || !material.ambient.is_finite()
            || material.ambient < 0.0
            || !material.specular.is_finite()
            || material.specular < 0.0
            || !material.shininess.is_finite()
            || material.shininess < 1.0
        {
            return Err(SceneError("invalid light or material parameters".into()));
        }
        let requested = scene.objects.len().max(1);
        if requested as u64
            > device.limits().max_buffer_size / std::mem::size_of::<ObjectUniform>() as u64
            || scene.objects.len() > u32::MAX as usize
        {
            return Err(SceneError(
                "too many object instances for this device".into(),
            ));
        }
        self.staging.clear();
        for object in &scene.objects {
            if meshes.get(object.mesh).is_none() {
                return Err(SceneError("unknown mesh handle".into()));
            }
            self.staging
                .push(ObjectUniform::from_transform(object.transform)?);
        }
        if requested > self.object_capacity {
            // Grow only when necessary. Mesh buffers themselves are never reuploaded.
            self.object_capacity = requested;
            self.objects = object_buffer(device, self.object_capacity);
        }
        if !self.staging.is_empty() {
            queue.write_buffer(&self.objects, 0, bytemuck::cast_slice(&self.staging));
        }
        let uniform = SceneUniform {
            view_projection: view_projection.to_cols_array_2d(),
            camera_position: scene.camera.eye.extend(1.).to_array(),
            light_direction: scene.light.direction.normalize().extend(0.).to_array(),
            light_color: [
                scene.light.color[0],
                scene.light.color[1],
                scene.light.color[2],
                0.,
            ],
            color_ambient: [
                material.color[0],
                material.color[1],
                material.color[2],
                material.ambient,
            ],
            specular_shininess: [material.specular, material.shininess, 0., 0.],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        Ok(())
    }

    /// Call after prepare with the same scene and mesh store.
    pub fn draw<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        meshes: &'pass MeshAssets,
        scene: &'pass Scene,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(1, self.objects.slice(..));
        for (instance, object) in scene.objects.iter().enumerate() {
            let mesh = meshes
                .get(object.mesh)
                .expect("prepare validates mesh handles");
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, instance as u32..instance as u32 + 1);
        }
    }
}

fn object_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Object transforms"),
        size: (count * std::mem::size_of::<ObjectUniform>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonuniform_scale_preserves_normal_perpendicularity() {
        let transform = Mat4::from_rotation_y(0.7) * Mat4::from_scale(Vec3::new(2., 3., 0.5));
        let raw = ObjectUniform::from_transform(transform).unwrap();
        let normal_matrix = Mat3::from_cols(
            Vec3::from_slice(&raw.normal[0]),
            Vec3::from_slice(&raw.normal[1]),
            Vec3::from_slice(&raw.normal[2]),
        );
        let tangent = transform.transform_vector3(Vec3::new(1., 1., 0.));
        let normal = normal_matrix * Vec3::new(-1., 1., 0.);
        assert!(tangent.dot(normal).abs() < 1e-5);
        assert!(ObjectUniform::from_transform(Mat4::ZERO).is_err());
    }

    #[test]
    fn perspective_uses_webgpu_depth_range() {
        let camera = Camera {
            eye: Vec3::ZERO,
            target: -Vec3::Z,
            near: 1.,
            far: 10.,
            ..Default::default()
        };
        let projection = camera.view_projection(1.).unwrap();
        assert!(projection.project_point3(Vec3::new(0., 0., -1.)).z.abs() < 1e-6);
        assert!((projection.project_point3(Vec3::new(0., 0., -10.)).z - 1.).abs() < 1e-6);
        assert!(
            Camera {
                vertical_fov: 0.,
                ..camera
            }
            .view_projection(1.)
            .is_err()
        );
    }
}
