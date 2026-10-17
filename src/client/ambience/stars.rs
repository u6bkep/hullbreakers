//! The stars: a catalogue grown from the ambience's `stars` (and its band,
//! which they gather along), the same on every peer, drawn as small quads
//! at infinity (`stars.wgsl`). One mesh for them all; the vertex shader
//! sizes each in pixels and hides those behind a body or the sun.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{STAR_SHADER, SkyUniform};
use crate::sim::ambience::{Band, Stars};

/// A star of the catalogue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    /// Toward it.
    pub direction: Vec3,
    /// Its colour times its brightness (linear, sky units).
    pub light: Vec3,
    /// Its core's radius (px).
    pub radius: f32,
    /// How far its glint reaches (px; 0: none).
    pub glint: f32,
}

/// The faintest star, as a share of the brightest.
const FAINTEST: f32 = 0.025;

/// Every star, faintest first.
pub fn catalogue(stars: &Stars, band: Option<&Band>) -> Vec<Star> {
    let mut rng = ChaCha8Rng::seed_from_u64(stars.seed);
    // A sky the validator would refuse still draws (the preview shows a
    // file as it's saved): as many stars as are allowed, white if it names
    // no colours.
    let count = stars.count.min(crate::sim::ambience::MAX_STARS);
    let white = [crate::sim::ambience::Rgb::new(255, 255, 255)];
    let colours: &[crate::sim::ambience::Rgb] = if stars.colors.is_empty() { &white } else { &stars.colors };
    let mut out = Vec::with_capacity(count as usize);
    let banded = band.map_or(0.0, |_| stars.banded);
    // The band's frame: its pole, and two ways round its plane.
    let frame = band.map(|b| {
        let pole = b.pole.normalize_or(Vec3::Y);
        let bulge = (b.bulge - pole * b.bulge.dot(pole)).normalize_or(pole.any_orthonormal_vector());
        (pole, bulge, pole.cross(bulge), (b.width / 2.0).to_radians())
    });
    for _ in 0..count {
        let direction = match frame {
            Some((pole, bulge, across, width)) if rng.random::<f32>() < banded => {
                // Gathered toward the band's plane, and more toward its bulge.
                let along: f32 = if rng.random_bool(0.3) {
                    rng.random_range(-0.7..0.7f32)
                } else {
                    rng.random_range(0.0..std::f32::consts::TAU)
                };
                let spread: f32 = rng.random_range(-1.0..1.0f32) + rng.random_range(-1.0..1.0f32);
                let lat = spread * width * 0.7;
                (bulge * along.cos() + across * along.sin()) * lat.cos() + pole * lat.sin()
            }
            _ => {
                let z: f32 = rng.random_range(-1.0..1.0);
                let a: f32 = rng.random_range(0.0..std::f32::consts::TAU);
                let r = (1.0 - z * z).sqrt();
                Vec3::new(r * a.cos(), z, r * a.sin())
            }
        };
        // Most are faint; a few are bright.
        let share = FAINTEST + (1.0 - FAINTEST) * rng.random::<f32>().powi(8);
        let colour = colours[rng.random_range(0..colours.len())].linear();
        out.push(Star {
            direction,
            light: colour * stars.brightness * share,
            radius: 0.5 + 0.9 * share.sqrt(),
            glint: 0.0,
        });
    }
    out.sort_by(|a, b| a.light.length_squared().total_cmp(&b.light.length_squared()));
    // The brightest get glints, longer the brighter.
    for (i, star) in out.iter_mut().rev().take(stars.glints as usize).enumerate() {
        let rank = 1.0 - i as f32 / stars.glints.max(1) as f32;
        star.glint = 5.0 + 9.0 * rank;
    }
    out
}

/// The catalogue as one mesh: a quad per star (see `stars.wgsl` for the
/// attributes).
pub fn mesh(stars: &[Star]) -> Mesh {
    let corners = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
    let mut directions = Vec::with_capacity(stars.len() * 4);
    let mut quad = Vec::with_capacity(stars.len() * 4);
    let mut sizes = Vec::with_capacity(stars.len() * 4);
    let mut colours = Vec::with_capacity(stars.len() * 4);
    let mut indices = Vec::with_capacity(stars.len() * 6);
    for (i, star) in stars.iter().enumerate() {
        for corner in corners {
            directions.push(star.direction.to_array());
            quad.push(corner.to_array());
            sizes.push([star.radius, star.glint]);
            colours.push([star.light.x, star.light.y, star.light.z, 1.0]);
        }
        let base = i as u32 * 4;
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, directions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, quad)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, sizes)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
        .with_inserted_indices(Indices::U32(indices))
}

/// The stars' look: the sky (for what hides them) and the scale to cd/m².
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct StarMaterial {
    #[uniform(0)]
    pub sky: SkyUniform,
    /// x: cd/m² per sky unit.
    #[uniform(1)]
    pub params: Vec4,
}

impl Material for StarMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(STAR_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(STAR_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::ambience::Ambience;

    #[test]
    fn the_catalogue_is_the_same_everywhere() {
        let sky = Ambience::reference();
        let a = catalogue(&sky.stars, sky.band.as_ref());
        let b = catalogue(&sky.stars, sky.band.as_ref());
        assert_eq!(a, b);
        assert_eq!(a.len(), sky.stars.count as usize);
        assert_eq!(a.iter().filter(|s| s.glint > 0.0).count(), sky.stars.glints as usize);
        assert!(a.iter().all(|s| (s.direction.length() - 1.0).abs() < 1e-3));
    }

    /// The preview draws a file as it's saved, mistakes and all.
    #[test]
    fn a_sky_the_validator_refuses_still_draws() {
        let mut sky = Ambience::reference();
        sky.stars.colors.clear();
        sky.stars.count = u32::MAX;
        let stars = catalogue(&sky.stars, sky.band.as_ref());
        assert_eq!(stars.len(), crate::sim::ambience::MAX_STARS as usize);
    }

    #[test]
    fn banded_stars_gather_along_the_band() {
        let mut sky = Ambience::reference();
        let band = sky.band.clone().unwrap();
        let near = |stars: &[Star]| {
            let pole = band.pole.normalize();
            stars.iter().filter(|s| s.direction.dot(pole).abs() < (band.width / 2.0).to_radians().sin()).count() as f32 / stars.len() as f32
        };
        sky.stars.banded = 0.0;
        let even = near(&catalogue(&sky.stars, Some(&band)));
        sky.stars.banded = 0.8;
        let gathered = near(&catalogue(&sky.stars, Some(&band)));
        assert!(gathered > even * 2.0, "{gathered} vs {even}");
    }
}
