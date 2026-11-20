//! The board as one piece of wood, as geometry: its outline round the cells, the slab with a
//! rounded top edge, flat cell shapes, and the shading laid over a cell along a goal's edge. No
//! systems here; `board.rs` puts the meshes on the table and says what each is made of.
//!
//! Points on the table are `Vec2`s of (x, z). Outlines run counter-clockwise in those axes, so
//! an edge's outward normal is its direction turned clockwise.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// The corners of a cell's top, around its middle: a hexagon with points to the far and near
/// sides, a square, or (for a round spot) a circle's worth of points.
pub fn cell_corners(shape: crate::board::CellShape, size: f32) -> Vec<Vec2> {
    use crate::board::CellShape;
    match shape {
        CellShape::Hexagon => {
            let r = size / 3f32.sqrt();
            (0..6)
                .map(|i| {
                    let a = FRAC_PI_2 + i as f32 * TAU / 6.0;
                    Vec2::new(r * a.cos(), -r * a.sin())
                })
                .collect()
        }
        CellShape::Square => {
            let h = size / 2.0;
            vec![
                Vec2::new(-h, -h),
                Vec2::new(-h, h),
                Vec2::new(h, h),
                Vec2::new(h, -h),
            ]
        }
        CellShape::Spot => (0..16)
            .map(|i| Vec2::from_angle(-(i as f32) * TAU / 16.0) * size * 0.3)
            .collect(),
    }
}

/// The convex hull of points, counter-clockwise (Andrew's monotone chain).
pub fn hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut p: Vec<Vec2> = points.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| a.distance(*b) < 1e-6);
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<Vec2> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 1e-9 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 1e-9 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The outward normal of the edge from `a` to `b` of a counter-clockwise outline.
fn outward(a: Vec2, b: Vec2) -> Vec2 {
    let d = (b - a).normalize_or_zero();
    Vec2::new(d.y, -d.x)
}

/// A convex outline grown by `margin` all round, its corners rounded with that radius: points
/// with their outward normals, counter-clockwise, `steps` segments round each corner.
pub fn grown(hull: &[Vec2], margin: f32, steps: usize) -> Vec<(Vec2, Vec2)> {
    let n = hull.len();
    let mut out: Vec<(Vec2, Vec2)> = Vec::new();
    for i in 0..n {
        let (prev, here, next) = (hull[(i + n - 1) % n], hull[i], hull[(i + 1) % n]);
        let from = outward(prev, here).to_angle();
        let mut to = outward(here, next).to_angle();
        while to < from {
            to += TAU;
        }
        for k in 0..=steps {
            let normal = Vec2::from_angle(from + (to - from) * k as f32 / steps as f32);
            let point = here + normal * margin;
            if out.last().is_none_or(|(p, _)| p.distance(point) > 1e-6) {
                out.push((point, normal));
            }
        }
    }
    out
}

/// The slab's top edge: how high its top is, and the radius it is rounded with.
#[derive(Debug, Clone, Copy)]
pub struct Rim {
    pub height: f32,
    pub radius: f32,
    pub steps: usize,
}

impl Rim {
    /// Across the side and round the edge, from the table up to the top: how far in from the
    /// outline, how high, and the normal's (outward, up) parts.
    fn profile(&self) -> Vec<(f32, f32, Vec2)> {
        let r = self.radius;
        let mut out = vec![(0.0, 0.0, Vec2::X)];
        for k in 0..=self.steps {
            let t = k as f32 / self.steps as f32 * FRAC_PI_2;
            out.push((
                r * (1.0 - t.cos()),
                self.height - r + r * t.sin(),
                Vec2::new(t.cos(), t.sin()),
            ));
        }
        out
    }
}

/// Builds a mesh from triangles, each turned to face the way its normals do.
#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Builder {
    fn vertex(&mut self, p: Vec3, n: Vec3) -> u32 {
        self.positions.push(p.to_array());
        self.normals.push(n.to_array());
        (self.positions.len() - 1) as u32
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        let p = |i: u32| Vec3::from_array(self.positions[i as usize]);
        let n = Vec3::from_array(self.normals[a as usize])
            + Vec3::from_array(self.normals[b as usize])
            + Vec3::from_array(self.normals[c as usize]);
        if (p(b) - p(a)).cross(p(c) - p(a)).dot(n) >= 0.0 {
            self.indices.extend([a, b, c]);
        } else {
            self.indices.extend([a, c, b]);
        }
    }

    fn mesh(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_indices(Indices::U32(self.indices));
        if !self.colours.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colours);
        }
        mesh
    }
}

/// The slab's flat top, inside its rounded edge.
pub fn top(outline: &[(Vec2, Vec2)], rim: &Rim) -> Mesh {
    let mut b = Builder::default();
    let inner: Vec<Vec2> = outline.iter().map(|(p, n)| *p - *n * rim.radius).collect();
    let middle = inner.iter().sum::<Vec2>() / inner.len().max(1) as f32;
    let at = |v: Vec2| Vec3::new(v.x, rim.height, v.y);
    let centre = b.vertex(at(middle), Vec3::Y);
    let ring: Vec<u32> = inner.iter().map(|v| b.vertex(at(*v), Vec3::Y)).collect();
    for i in 0..ring.len() {
        b.triangle(centre, ring[i], ring[(i + 1) % ring.len()]);
    }
    b.mesh()
}

/// The side and the rounded edge along these edges of the outline (edge i runs from point i to
/// point i + 1), as one mesh.
pub fn side(outline: &[(Vec2, Vec2)], rim: &Rim, edges: &[usize]) -> Mesh {
    let mut b = Builder::default();
    let profile = rim.profile();
    let n = outline.len();
    for &i in edges {
        let column = |b: &mut Builder, (p, normal): (Vec2, Vec2)| -> Vec<u32> {
            profile
                .iter()
                .map(|(inset, y, dir)| {
                    let at = p - normal * *inset;
                    let n3 = Vec3::new(normal.x * dir.x, dir.y, normal.y * dir.x);
                    b.vertex(Vec3::new(at.x, *y, at.y), n3)
                })
                .collect()
        };
        let a = column(&mut b, outline[i]);
        let c = column(&mut b, outline[(i + 1) % n]);
        for k in 0..profile.len() - 1 {
            b.triangle(a[k], c[k], c[k + 1]);
            b.triangle(a[k], c[k + 1], a[k + 1]);
        }
    }
    b.mesh()
}

/// Flat cell shapes (`corners` round each centre), as one mesh facing up at `height`.
pub fn flat_cells(centres: &[Vec2], corners: &[Vec2], height: f32) -> Mesh {
    let mut b = Builder::default();
    for c in centres {
        let middle = b.vertex(Vec3::new(c.x, height, c.y), Vec3::Y);
        let ring: Vec<u32> = corners
            .iter()
            .map(|k| b.vertex(Vec3::new(c.x + k.x, height, c.y + k.y), Vec3::Y))
            .collect();
        for i in 0..ring.len() {
            b.triangle(middle, ring[i], ring[(i + 1) % ring.len()]);
        }
    }
    b.mesh()
}

/// The lines of a board played on its points: one between each two positions a pitch apart (so a
/// square grid's are its rows and columns, and a hexagonal one's run three ways), `width` wide
/// and `edge_width` along the board's edge, where the positions have fewer neighbours than most.
/// Each runs half its width past its ends, so lines meet cleanly at the crossings.
pub fn grid_lines(points: &[Vec2], pitch: f32, width: f32, edge_width: f32, height: f32) -> Mesh {
    let near = |a: Vec2, b: Vec2| (a.distance(b) - pitch).abs() < pitch * 0.05;
    let mut neighbours = vec![0usize; points.len()];
    let mut pairs = Vec::new();
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            if near(points[i], points[j]) {
                neighbours[i] += 1;
                neighbours[j] += 1;
                pairs.push((i, j));
            }
        }
    }
    let most = neighbours.iter().copied().max().unwrap_or(0);
    let mut b = Builder::default();
    for (i, j) in pairs {
        let on_edge = neighbours[i] < most && neighbours[j] < most;
        let w = if on_edge { edge_width } else { width } / 2.0;
        let along = (points[j] - points[i]).normalize();
        let across = along.perp() * w;
        let (from, to) = (points[i] - along * w, points[j] + along * w);
        let at = |v: Vec2| Vec3::new(v.x, height, v.y);
        let q = [
            b.vertex(at(from - across), Vec3::Y),
            b.vertex(at(to - across), Vec3::Y),
            b.vertex(at(to + across), Vec3::Y),
            b.vertex(at(from + across), Vec3::Y),
        ];
        b.triangle(q[0], q[1], q[2]);
        b.triangle(q[0], q[2], q[3]);
    }
    b.mesh()
}

/// How dark a cell's shading is at a point `d` in from the side it is drawn towards, where it is
/// `strength`, fading smoothly to nothing `fade` in.
pub fn shade_at(d: f32, fade: f32, strength: f32) -> f32 {
    let t = (1.0 - d / fade).clamp(0.0, 1.0);
    strength * t * t * (3.0 - 2.0 * t)
}

/// A cell's top, shaded towards one of its sides in `colour`: as strong as `strength` at that
/// side (`apothem` from the middle, facing `toward`), and fading to nothing `fade` in from it.
/// Rings of points, close enough for the fade to look smooth across the cell.
pub fn shading(
    centre: Vec3,
    corners: &[Vec2],
    toward: Vec2,
    apothem: f32,
    fade: f32,
    colour: LinearRgba,
    strength: f32,
) -> Mesh {
    // Close together, so that a short fade near the side is still smooth.
    const RINGS: usize = 12;
    const PER_SIDE: usize = 4;
    let mut b = Builder::default();
    let add = |b: &mut Builder, v: Vec2| -> u32 {
        let alpha = shade_at(apothem - v.dot(toward), fade, strength);
        b.colours
            .push([colour.red, colour.green, colour.blue, alpha]);
        b.vertex(centre + Vec3::new(v.x, 0.0, v.y), Vec3::Y)
    };
    // The outline of the cell, its sides cut into equal pieces.
    let n = corners.len();
    let edge: Vec<Vec2> = (0..n)
        .flat_map(|i| {
            let (a, c) = (corners[i], corners[(i + 1) % n]);
            (0..PER_SIDE).map(move |k| a.lerp(c, k as f32 / PER_SIDE as f32))
        })
        .collect();
    let middle = add(&mut b, Vec2::ZERO);
    let mut inner: Vec<u32> = Vec::new();
    for ring in 1..=RINGS {
        let scale = ring as f32 / RINGS as f32;
        let outer: Vec<u32> = edge.iter().map(|v| add(&mut b, *v * scale)).collect();
        let m = outer.len();
        for i in 0..m {
            let j = (i + 1) % m;
            if inner.is_empty() {
                b.triangle(middle, outer[i], outer[j]);
            } else {
                b.triangle(inner[i], outer[i], outer[j]);
                b.triangle(inner[i], outer[j], inner[j]);
            }
        }
        inner = outer;
    }
    b.mesh()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::CellShape;

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        p.iter().map(|v| Vec3::from_array(*v)).collect()
    }

    /// Every triangle faces the way its vertices' normals do.
    fn faces_its_normals(mesh: &Mesh) {
        let p = positions(mesh);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals");
        };
        let Some(Indices::U32(idx)) = mesh.indices() else {
            panic!("indices");
        };
        for t in idx.chunks(3) {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let normal: Vec3 = t.iter().map(|i| Vec3::from_array(n[*i as usize])).sum();
            assert!((b - a).cross(c - a).dot(normal) >= 0.0);
        }
    }

    #[test]
    fn the_hull_of_a_square_of_points_and_one_inside() {
        let h = hull(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.5, 0.5),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ]);
        assert_eq!(h.len(), 4);
        let area: f32 = (0..4).map(|i| h[i].perp_dot(h[(i + 1) % 4])).sum::<f32>() / 2.0;
        assert!((area - 1.0).abs() < 1e-5, "counter-clockwise: {area}");
    }

    #[test]
    fn an_outline_grown_round_a_square_keeps_its_distance_and_faces_out() {
        let h = hull(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ]);
        let o = grown(&h, 0.1, 4);
        assert_eq!(o.len(), 4 * 5);
        let middle = Vec2::splat(0.5);
        for (p, n) in &o {
            assert!((n.length() - 1.0).abs() < 1e-5);
            assert!(n.dot(*p - middle) > 0.0, "faces out");
            let d = (*p - p.clamp(Vec2::ZERO, Vec2::ONE)).length();
            assert!((d - 0.1).abs() < 1e-4, "{p} is {d} away");
        }
    }

    #[test]
    fn the_slab_is_as_high_as_its_rim_and_faces_out_and_up() {
        let h = hull(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ]);
        let o = grown(&h, 0.1, 4);
        let rim = Rim {
            height: 0.03,
            radius: 0.005,
            steps: 4,
        };
        let flat = top(&o, &rim);
        faces_its_normals(&flat);
        assert!(positions(&flat).iter().all(|p| (p.y - 0.03).abs() < 1e-6));
        let all: Vec<usize> = (0..o.len()).collect();
        let round = side(&o, &rim, &all);
        faces_its_normals(&round);
        let ys: Vec<f32> = positions(&round).iter().map(|p| p.y).collect();
        assert!(ys.iter().any(|y| y.abs() < 1e-6), "down to the table");
        assert!(ys.iter().all(|y| *y <= 0.03 + 1e-6));
        assert!(
            side(&o, &rim, &all[..3]).count_vertices() < round.count_vertices(),
            "a part of the edge is a part of the mesh"
        );
    }

    #[test]
    fn a_hexagon_cell_has_points_far_and_near() {
        let c = cell_corners(CellShape::Hexagon, 1.0);
        assert_eq!(c.len(), 6);
        assert!(c.iter().any(|v| (v.y + 1.0 / 3f32.sqrt()).abs() < 1e-5));
        assert!(
            c.iter().all(|v| v.x.abs() <= 0.5 + 1e-5),
            "flat sides one pitch apart"
        );
        let ink = flat_cells(&[Vec2::ZERO, Vec2::X], &c, 0.01);
        faces_its_normals(&ink);
        assert_eq!(ink.count_vertices(), 14);
    }

    #[test]
    fn a_square_grid_has_its_rows_and_columns_and_a_thicker_edge() {
        let points: Vec<Vec2> = (0..3)
            .flat_map(|y| (0..3).map(move |x| Vec2::new(x as f32, y as f32)))
            .collect();
        let mesh = grid_lines(&points, 1.0, 0.1, 0.2, 0.5);
        faces_its_normals(&mesh);
        // Three rows and three columns of two segments each: twelve quads.
        assert_eq!(mesh.count_vertices(), 12 * 4);
        let p = positions(&mesh);
        assert!(p.iter().all(|v| (v.y - 0.5).abs() < 1e-6));
        // Each quad's width is the distance from its first corner to its last: the four
        // segments to the middle point are inside the board, the other eight along its edge.
        let widths: Vec<f32> = p.chunks(4).map(|q| q[0].distance(q[3])).collect();
        assert_eq!(widths.iter().filter(|w| (*w - 0.1).abs() < 1e-5).count(), 4);
        assert_eq!(widths.iter().filter(|w| (*w - 0.2).abs() < 1e-5).count(), 8);
    }

    #[test]
    fn shading_is_strongest_at_its_side_and_gone_further_in() {
        assert!((shade_at(0.0, 1.0, 0.8) - 0.8).abs() < 1e-6);
        assert_eq!(shade_at(1.0, 1.0, 0.8), 0.0);
        assert_eq!(shade_at(2.0, 1.0, 0.8), 0.0);
        assert!(shade_at(0.3, 1.0, 0.8) > shade_at(0.6, 1.0, 0.8));

        let corners = cell_corners(CellShape::Hexagon, 1.0);
        let mesh = shading(
            Vec3::ZERO,
            &corners,
            Vec2::X,
            0.5,
            0.7,
            LinearRgba::BLACK,
            0.8,
        );
        faces_its_normals(&mesh);
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colours)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colours");
        };
        let p = positions(&mesh);
        let alpha_at = |x: f32| {
            p.iter()
                .zip(colours)
                .filter(|(v, _)| (v.x - x).abs() < 0.02)
                .map(|(_, c)| c[3])
                .fold(0.0f32, f32::max)
        };
        assert!(alpha_at(0.5) > 0.7, "dark at the side");
        assert_eq!(alpha_at(-0.5), 0.0, "clear across the cell");
    }
}
