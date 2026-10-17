//! Bodies and the sun's disc: impostors at infinity (`bodies.wgsl`), a quad
//! each, shaded per pixel from the same functions the light cubemap is
//! baked with.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

use super::{BODY_SHADER, SkyUniform};

/// The impostor slot for the sun's disc (after the bodies').
pub const SUN_SLOT: usize = crate::sim::ambience::MAX_BODIES;

/// A body's look: the sky (it's lit by the sun in it), the scale to cd/m²,
/// and which body it is.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct BodyMaterial {
    #[uniform(0)]
    pub sky: SkyUniform,
    /// x: cd/m² per sky unit; y: which body, or [`SUN_SLOT`] for the sun.
    #[uniform(1)]
    pub params: Vec4,
}

impl Material for BodyMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(BODY_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(BODY_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// Behind every other transparent thing (they're all at the origin, so
    /// they'd otherwise sort among the world's effects and paint over those
    /// beyond it), the sun's glow behind the bodies.
    fn depth_bias(&self) -> f32 {
        if self.params.y as usize == SUN_SLOT { -2.0e9 } else { -1.0e9 }
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
        let vertex_layout = layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}
