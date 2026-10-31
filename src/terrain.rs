//! Triangle BVH for arbitrary static geometry; no heightfield assumption.

use crate::rendering::MeshData;
use glam::{Mat4, Vec3};

#[derive(Clone, Copy)]
struct Triangle {
    a: Vec3,
    b: Vec3,
    c: Vec3,
}
#[derive(Clone, Copy)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}
struct Node {
    bounds: Aabb,
    children: Option<[usize; 2]>,
    start: usize,
    end: usize,
}

pub struct Terrain {
    triangles: Vec<Triangle>,
    nodes: Vec<Node>,
    pub min: Vec3,
    pub max: Vec3,
}

impl Terrain {
    /// Compatibility fingerprint of transformed collision geometry; not a security signature.
    pub fn signature(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for triangle in &self.triangles {
            for point in [triangle.a, triangle.b, triangle.c] {
                for value in point.to_array() {
                    for byte in value.to_bits().to_le_bytes() {
                        hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
                    }
                }
            }
        }
        hash
    }
    pub fn new(data: &MeshData, transform: Mat4) -> Self {
        let triangles: Vec<_> = data
            .indices
            .chunks_exact(3)
            .map(|i| Triangle {
                a: transform
                    .transform_point3(Vec3::from_array(data.vertices[i[0] as usize].position)),
                b: transform
                    .transform_point3(Vec3::from_array(data.vertices[i[1] as usize].position)),
                c: transform
                    .transform_point3(Vec3::from_array(data.vertices[i[2] as usize].position)),
            })
            .collect();
        let mut terrain = Self {
            triangles,
            nodes: Vec::new(),
            min: Vec3::ZERO,
            max: Vec3::ZERO,
        };
        if !terrain.triangles.is_empty() {
            terrain.build(0, terrain.triangles.len());
            terrain.min = terrain.nodes[0].bounds.min;
            terrain.max = terrain.nodes[0].bounds.max;
        }
        terrain
    }
    fn build(&mut self, start: usize, end: usize) -> usize {
        let mut bounds = Aabb {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        };
        for triangle in &self.triangles[start..end] {
            for p in [triangle.a, triangle.b, triangle.c] {
                bounds.min = bounds.min.min(p);
                bounds.max = bounds.max.max(p);
            }
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            children: None,
            start,
            end,
        });
        if end - start > 8 {
            let span = bounds.max - bounds.min;
            let axis = if span.x >= span.y && span.x >= span.z {
                0
            } else if span.y >= span.z {
                1
            } else {
                2
            };
            self.triangles[start..end].sort_unstable_by(|a, b| {
                ((a.a + a.b + a.c)[axis]).total_cmp(&((b.a + b.b + b.c)[axis]))
            });
            let mid = (start + end) / 2;
            let left = self.build(start, mid);
            let right = self.build(mid, end);
            self.nodes[index].children = Some([left, right]);
        }
        index
    }
    /// Return the nearest segment fraction (0..1), including vertical faces/overhangs.
    pub fn segment_hit(&self, start: Vec3, end: Vec3) -> Option<f32> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut closest = 1.00001;
        self.ray_node(0, start, end - start, &mut closest);
        (closest <= 1.).then_some(closest)
    }
    fn ray_node(&self, index: usize, origin: Vec3, delta: Vec3, closest: &mut f32) {
        let node = &self.nodes[index];
        if !segment_aabb(origin, delta, node.bounds, *closest) {
            return;
        }
        if let Some(children) = node.children {
            for child in children {
                self.ray_node(child, origin, delta, closest);
            }
        } else {
            for triangle in &self.triangles[node.start..node.end] {
                if let Some(t) = segment_triangle(origin, delta, *triangle) {
                    *closest = closest.min(t);
                }
            }
        }
    }
    pub fn overlaps_sphere(&self, center: Vec3, radius: f32) -> bool {
        !self.nodes.is_empty() && self.sphere_node(0, center, radius * radius)
    }
    fn sphere_node(&self, index: usize, center: Vec3, radius_sq: f32) -> bool {
        let node = &self.nodes[index];
        if center.distance_squared(center.clamp(node.bounds.min, node.bounds.max)) > radius_sq {
            return false;
        }
        if let Some(children) = node.children {
            children
                .into_iter()
                .any(|i| self.sphere_node(i, center, radius_sq))
        } else {
            self.triangles[node.start..node.end]
                .iter()
                .any(|t| center.distance_squared(closest_point(center, *t)) <= radius_sq)
        }
    }
    pub fn ground_at(&self, x: f32, z: f32) -> f32 {
        let top = Vec3::new(x, self.max.y + 100., z);
        let bottom = Vec3::new(x, self.min.y - 100., z);
        self.segment_hit(top, bottom)
            .map_or(self.min.y, |t| top.lerp(bottom, t).y)
    }
}

fn segment_aabb(origin: Vec3, delta: Vec3, bounds: Aabb, max_t: f32) -> bool {
    let mut near = 0_f32;
    let mut far = max_t.min(1.);
    for axis in 0..3 {
        if delta[axis].abs() < 1e-8 {
            if origin[axis] < bounds.min[axis] || origin[axis] > bounds.max[axis] {
                return false;
            }
        } else {
            let a = (bounds.min[axis] - origin[axis]) / delta[axis];
            let b = (bounds.max[axis] - origin[axis]) / delta[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return false;
            }
        }
    }
    true
}

fn segment_triangle(origin: Vec3, delta: Vec3, t: Triangle) -> Option<f32> {
    let edge1 = t.b - t.a;
    let edge2 = t.c - t.a;
    let cross = delta.cross(edge2);
    let determinant = edge1.dot(cross);
    if determinant.abs() < 1e-8 {
        return None;
    }
    let inv = determinant.recip();
    let offset = origin - t.a;
    let u = offset.dot(cross) * inv;
    if !(0. ..=1.).contains(&u) {
        return None;
    }
    let q = offset.cross(edge1);
    let v = delta.dot(q) * inv;
    if v < 0. || u + v > 1. {
        return None;
    }
    let hit = edge2.dot(q) * inv;
    (0. ..=1.).contains(&hit).then_some(hit)
}

fn closest_point(p: Vec3, t: Triangle) -> Vec3 {
    let ab = t.b - t.a;
    let ac = t.c - t.a;
    let ap = p - t.a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0. && d2 <= 0. {
        return t.a;
    }
    let bp = p - t.b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0. && d4 <= d3 {
        return t.b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        return t.a + ab * d1 / (d1 - d3);
    }
    let cp = p - t.c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0. && d5 <= d6 {
        return t.c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        return t.a + ac * d2 / (d2 - d6);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
        return t.b + (t.c - t.b) * (d4 - d3) / (d4 - d3 + d5 - d6);
    }
    let denominator = va + vb + vc;
    if denominator.abs() < 1e-12 {
        return t.a;
    }
    t.a + ab * vb / denominator + ac * vc / denominator
}

pub fn segment_sphere(start: Vec3, end: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let delta = end - start;
    let relative = start - center;
    let c = relative.length_squared() - radius * radius;
    if c <= 0. {
        return Some(0.);
    }
    let a = delta.length_squared();
    if a < 1e-10 {
        return None;
    }
    let b = relative.dot(delta);
    let discriminant = b * b - a * c;
    if discriminant < 0. {
        return None;
    }
    let t = (-b - discriminant.sqrt()) / a;
    (0. ..=1.).contains(&t).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::mesh::Vertex;
    #[test]
    fn arbitrary_vertical_geometry_is_collidable_and_occludes_radar() {
        let data = MeshData {
            vertices: vec![
                Vertex {
                    position: [0., -5., -5.],
                    normal: [1., 0., 0.],
                },
                Vertex {
                    position: [0., 5., -5.],
                    normal: [1., 0., 0.],
                },
                Vertex {
                    position: [0., 0., 5.],
                    normal: [1., 0., 0.],
                },
            ],
            indices: vec![0, 1, 2],
        };
        let terrain = Terrain::new(&data, Mat4::IDENTITY);
        assert!((terrain.segment_hit(-Vec3::X * 10., Vec3::X * 10.).unwrap() - 0.5).abs() < 1e-5);
        assert!(terrain.overlaps_sphere(Vec3::X, 1.1));
        assert!(!terrain.overlaps_sphere(Vec3::X * 2., 1.));
    }
    #[test]
    fn fast_projectile_does_not_tunnel_through_actor() {
        assert!(segment_sphere(-Vec3::Z * 100., Vec3::Z * 100., Vec3::ZERO, 5.).is_some());
        assert!(segment_sphere(-Vec3::Z, -Vec3::Z * 10., Vec3::ZERO, 0.5).is_none());
    }
}
