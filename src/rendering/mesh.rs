//! File decoding ends at MeshData. GPU storage accepts the same data from any packer.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use wgpu::util::DeviceExt;

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("mesh I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("GLB decode error: {0}")]
    Gltf(#[from] gltf::Error),
    #[error("invalid or unsupported mesh: {0}")]
    Invalid(String),
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

impl Vertex {
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    pub fn radius(self) -> f32 {
        (self.max - self.min).length() * 0.5
    }
}

/// Format-independent, indexed triangle data. CPU copies are released after loading.
#[derive(Debug, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Read geometry only. Textures and GLB material assignments are ignored.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, MeshError> {
        let source = gltf::Gltf::from_slice(bytes)?;
        let blob = source
            .blob
            .as_deref()
            .ok_or_else(|| invalid("expected a binary GLB"))?;
        for buffer in source.buffers() {
            if !matches!(buffer.source(), gltf::buffer::Source::Bin) {
                return Err(invalid("external geometry buffers are not supported"));
            }
            if buffer.length() > blob.len() {
                return Err(invalid("truncated binary geometry buffer"));
            }
        }
        let scene = source
            .default_scene()
            .or_else(|| source.scenes().next())
            .ok_or_else(|| invalid("GLB has no scene"))?;
        let mut mesh = Self::default();
        for node in scene.nodes() {
            mesh.read_node(node, Mat4::IDENTITY, blob)?;
        }
        mesh.bounds()?;
        Ok(mesh)
    }

    fn read_node(
        &mut self,
        node: gltf::Node<'_>,
        parent: Mat4,
        blob: &[u8],
    ) -> Result<(), MeshError> {
        if node.skin().is_some() {
            return Err(invalid("skinned meshes require a future animation path"));
        }
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                if primitive.morph_targets().next().is_some() {
                    return Err(invalid("morph targets are not supported"));
                }
                let reader = primitive.reader(|_| Some(blob));
                let positions: Vec<_> = reader
                    .read_positions()
                    .ok_or_else(|| invalid("primitive has no positions"))?
                    .collect();
                let normals = reader
                    .read_normals()
                    .map(|values| values.collect::<Vec<_>>());
                let source_indices: Vec<u32> = match reader.read_indices() {
                    Some(indices) => indices.into_u32().collect(),
                    None => (0..u32::try_from(positions.len())
                        .map_err(|_| invalid("too many vertices"))?)
                        .collect(),
                };
                let indices = triangle_indices(&source_indices, primitive.mode())?;
                self.append(positions, normals, indices, transform)?;
            }
        }
        for child in node.children() {
            self.read_node(child, transform, blob)?;
        }
        Ok(())
    }

    fn append(
        &mut self,
        positions: Vec<[f32; 3]>,
        normals: Option<Vec<[f32; 3]>>,
        mut indices: Vec<u32>,
        transform: Mat4,
    ) -> Result<(), MeshError> {
        if positions.is_empty() || indices.is_empty() || indices.len() % 3 != 0 {
            return Err(invalid("expected nonempty triangle geometry"));
        }
        if positions.iter().any(|p| !Vec3::from_array(*p).is_finite())
            || indices.iter().any(|&i| i as usize >= positions.len())
        {
            return Err(invalid("non-finite position or out-of-range index"));
        }
        let determinant = transform.determinant();
        if !transform.is_finite() || !determinant.is_finite() || determinant == 0.0 {
            return Err(invalid("singular or non-finite node transform"));
        }
        let normals = match normals {
            Some(values) if values.len() == positions.len() => values,
            Some(_) => return Err(invalid("normal and position counts differ")),
            None => generated_normals(&positions, &indices),
        };
        let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
        let base = u32::try_from(self.vertices.len()).map_err(|_| invalid("too many vertices"))?;
        let total = self
            .vertices
            .len()
            .checked_add(positions.len())
            .ok_or_else(|| invalid("mesh too large"))?;
        u32::try_from(total).map_err(|_| invalid("too many vertices"))?;
        if determinant < 0.0 {
            for triangle in indices.chunks_exact_mut(3) {
                triangle.swap(1, 2);
            }
        }
        for (position, normal) in positions.into_iter().zip(normals) {
            let position = transform.transform_point3(Vec3::from_array(position));
            let normal = (normal_matrix * Vec3::from_array(normal)).normalize_or_zero();
            if !position.is_finite() || !normal.is_finite() || normal == Vec3::ZERO {
                return Err(invalid("invalid transformed position or normal"));
            }
            self.vertices.push(Vertex {
                position: position.to_array(),
                normal: normal.to_array(),
            });
        }
        self.indices.extend(indices.into_iter().map(|i| base + i));
        Ok(())
    }

    pub fn bounds(&self) -> Result<Bounds, MeshError> {
        if self.vertices.is_empty() || self.indices.is_empty() || self.indices.len() % 3 != 0 {
            return Err(invalid("expected nonempty indexed triangles"));
        }
        u32::try_from(self.indices.len()).map_err(|_| invalid("too many indices"))?;
        if self
            .indices
            .iter()
            .any(|&i| i as usize >= self.vertices.len())
        {
            return Err(invalid("index outside vertex buffer"));
        }
        let mut bounds = Bounds {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        };
        for vertex in &self.vertices {
            let p = Vec3::from_array(vertex.position);
            let n = Vec3::from_array(vertex.normal);
            if !p.is_finite() || !n.is_finite() || n.length_squared() == 0.0 {
                return Err(invalid(
                    "vertices need finite positions and nonzero finite normals",
                ));
            }
            bounds.min = bounds.min.min(p);
            bounds.max = bounds.max.max(p);
        }
        Ok(bounds)
    }
}

fn invalid(message: &str) -> MeshError {
    MeshError::Invalid(message.into())
}

fn triangle_indices(source: &[u32], mode: gltf::mesh::Mode) -> Result<Vec<u32>, MeshError> {
    use gltf::mesh::Mode;
    match mode {
        Mode::Triangles if source.len() % 3 == 0 => Ok(source.to_vec()),
        Mode::TriangleStrip | Mode::TriangleFan if source.len() >= 3 => {
            let mut triangles = Vec::with_capacity((source.len() - 2) * 3);
            for i in 2..source.len() {
                triangles.extend_from_slice(&match mode {
                    Mode::TriangleFan => [source[0], source[i - 1], source[i]],
                    _ if i % 2 == 0 => [source[i - 2], source[i - 1], source[i]],
                    _ => [source[i - 1], source[i - 2], source[i]],
                });
            }
            Ok(triangles)
        }
        _ => Err(invalid(
            "only triangle lists, strips, and fans are supported",
        )),
    }
}

fn generated_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        let normal = (Vec3::from_array(positions[b]) - Vec3::from_array(positions[a]))
            .cross(Vec3::from_array(positions[c]) - Vec3::from_array(positions[a]));
        for i in [a, b, c] {
            normals[i] += normal;
        }
    }
    normals
        .into_iter()
        .map(|n| n.try_normalize().unwrap_or(Vec3::Y).to_array())
        .collect()
}

/// Stable, renderer-owned resource handle; never a raw GPU memory pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshHandle(usize);

pub struct GpuMesh {
    pub(super) vertex_buffer: wgpu::Buffer,
    pub(super) index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub bounds: Bounds,
    pub buffer_bytes: u64,
}

/// Append-only GPU resources. Loading the same canonical file reuses its handle.
#[derive(Default)]
pub struct MeshAssets {
    meshes: Vec<GpuMesh>,
    paths: HashMap<PathBuf, MeshHandle>,
}

impl MeshAssets {
    pub fn load(
        &mut self,
        device: &wgpu::Device,
        path: impl AsRef<Path>,
    ) -> Result<MeshHandle, MeshError> {
        let path = path.as_ref().canonicalize()?;
        if let Some(handle) = self.paths.get(&path) {
            return Ok(*handle);
        }
        let data = MeshData::from_glb(&std::fs::read(&path)?)?;
        let handle = self.upload(device, &path.to_string_lossy(), &data)?;
        self.paths.insert(path, handle);
        Ok(handle)
    }

    /// Future packed-file decoders can bypass GLB and upload this layout directly.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        label: &str,
        data: &MeshData,
    ) -> Result<MeshHandle, MeshError> {
        let bounds = data.bounds()?;
        let vertices = bytemuck::cast_slice(&data.vertices);
        let indices = bytemuck::cast_slice(&data.indices);
        let limit = device.limits().max_buffer_size;
        if vertices.len() as u64 > limit || indices.len() as u64 > limit {
            return Err(invalid("geometry exceeds the device buffer-size limit"));
        }
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: vertices,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: indices,
            usage: wgpu::BufferUsages::INDEX,
        });
        let handle = MeshHandle(self.meshes.len());
        self.meshes.push(GpuMesh {
            vertex_buffer,
            index_buffer,
            bounds,
            index_count: data.indices.len() as u32,
            buffer_bytes: (vertices.len() + indices.len()) as u64,
        });
        Ok(handle)
    }

    pub fn get(&self, handle: MeshHandle) -> Option<&GpuMesh> {
        self.meshes.get(handle.0)
    }
    pub fn len(&self) -> usize {
        self.meshes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }
    pub fn buffer_bytes(&self) -> u64 {
        self.meshes.iter().map(|mesh| mesh.buffer_bytes).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplied_glbs_decode_to_valid_geometry() {
        for file in [
            "f-16_fighter_jet.glb",
            "world_01.glb",
            "bullet.glb",
            "rocket.glb",
        ] {
            let bytes = std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join(file),
            )
            .unwrap();
            let mesh = MeshData::from_glb(&bytes).unwrap_or_else(|e| panic!("{file}: {e}"));
            let bounds = mesh.bounds().unwrap();
            assert!(bounds.radius() > 0.0, "{file}");
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| (Vec3::from_array(v.normal).length() - 1.0).abs() < 0.001)
            );
        }
    }

    #[test]
    fn mirrored_node_transforms_preserve_normal_and_winding_agreement() {
        let mut mesh = MeshData::default();
        mesh.append(
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            None,
            vec![0, 1, 2],
            Mat4::from_scale(Vec3::new(-2., 3., 1.)),
        )
        .unwrap();
        assert_eq!(mesh.indices, [0, 2, 1]);
        let points: Vec<_> = mesh
            .indices
            .iter()
            .map(|&i| Vec3::from_array(mesh.vertices[i as usize].position))
            .collect();
        let geometric_normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize();
        assert!(geometric_normal.dot(Vec3::from_array(mesh.vertices[0].normal)) > 0.999);
    }

    #[test]
    fn strip_winding_and_fan_topology_are_preserved() {
        assert_eq!(
            triangle_indices(&[0, 1, 2, 3], gltf::mesh::Mode::TriangleStrip).unwrap(),
            [0, 1, 2, 2, 1, 3]
        );
        assert_eq!(
            triangle_indices(&[0, 1, 2, 3], gltf::mesh::Mode::TriangleFan).unwrap(),
            [0, 1, 2, 0, 2, 3]
        );
    }

    #[test]
    fn malformed_data_is_rejected() {
        assert!(MeshData::from_glb(b"not a GLB").is_err());
        let mut mesh = MeshData::default();
        assert!(
            mesh.append(vec![[0., 0., 0.]; 3], None, vec![0, 1, 9], Mat4::IDENTITY)
                .is_err()
        );
        assert!(
            mesh.append(vec![[0., 0., 0.]; 3], None, vec![0, 1, 2], Mat4::ZERO)
                .is_err()
        );
    }
}
