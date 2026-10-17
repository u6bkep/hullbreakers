//! The backdrop's bake: whenever the sky changes, its two cubemaps are
//! drawn on the GPU from `sky.wgsl`, a face at a time:
//!
//! - the **display** map (`DISPLAY_FACE`² a face, `Rg11b10Ufloat`, HDR),
//!   what Bevy's `Skybox` shows behind everything: the band and nebulae;
//! - the **light** map (`LIGHT_FACE`², `Rgba16Float`, a power of two as
//!   `GeneratedEnvironmentMapLight` needs), the same with the bodies over
//!   it, which the cameras' environment light is filtered from.
//!
//! The main world asks for a bake by bumping [`SkyBake::generation`]; the
//! render world draws it once the cubemaps and the pipelines are ready (the
//! first time, the shaders may take a few frames to compile) and says so
//! in [`SkyBake::baked`], which the main world waits on before it filters
//! the light again (see `super::refilter_when_baked`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::uniform_buffer;
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState,
    ColorWrites, Extent3d, FragmentState, LoadOp, Operations, PipelineCache, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages, ShaderType, StoreOp, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor, TextureViewDimension, UniformBuffer,
};
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};

use super::{BAKE_SHADER, SkyUniform};

/// The display map's face (texels): about the screen's own density at
/// 1440p and an 85° field of view, so the tones' edges stay crisp.
pub const DISPLAY_FACE: u32 = 1536;
const DISPLAY_FORMAT: TextureFormat = TextureFormat::Rg11b10Ufloat;
/// The light map's face: the light is filtered smooth anyway.
pub const LIGHT_FACE: u32 = 256;
const LIGHT_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// What's to be baked, and what was. Main world; the render world reads it.
#[derive(Resource, Clone)]
pub struct SkyBake {
    pub display: Handle<Image>,
    pub light: Handle<Image>,
    pub sky: SkyUniform,
    /// Bumped to ask for a bake of `sky`.
    pub generation: u32,
    /// The last generation baked (written by the render world).
    pub baked: Arc<AtomicU32>,
}

impl SkyBake {
    pub fn baked(&self) -> u32 {
        self.baked.load(Ordering::Acquire)
    }
}

/// An empty cubemap of `face`² texels, to be drawn into.
pub fn target_cubemap(face: u32, format: TextureFormat) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: face,
            height: face,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        format,
        // Kept in the main world too: the environment light's generator
        // reads its size there each time it's asked to filter again.
        RenderAssetUsages::all(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

pub fn display_cubemap() -> Image {
    target_cubemap(DISPLAY_FACE, DISPLAY_FORMAT)
}

pub fn light_cubemap() -> Image {
    target_cubemap(LIGHT_FACE, LIGHT_FORMAT)
}

pub struct SkyBakePlugin;

impl Plugin for SkyBakePlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<BakeJob>()
            .add_systems(RenderStartup, init_bake_pipelines)
            .add_systems(ExtractSchedule, extract_bake)
            .add_systems(
                Render,
                bake_sky.after(RenderSystems::PrepareBindGroups).before(RenderSystems::Render),
            );
    }
}

/// One face of one cubemap (see `bake.wgsl`).
#[derive(Clone, Copy, ShaderType)]
struct FaceUniform {
    face: u32,
    light: u32,
    texel: f32,
    pad: f32,
}

#[derive(Resource)]
struct BakePipelines {
    layout: BindGroupLayoutDescriptor,
    display: CachedRenderPipelineId,
    light: CachedRenderPipelineId,
}

/// The bake waiting to be drawn, in the render world.
#[derive(Resource, Default)]
struct BakeJob(Option<SkyBake>);

fn init_bake_pipelines(mut commands: Commands, cache: Res<PipelineCache>, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "sky_bake",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (uniform_buffer::<SkyUniform>(false), uniform_buffer::<FaceUniform>(false)),
        ),
    );
    let pipeline = |format: TextureFormat| RenderPipelineDescriptor {
        label: Some("sky_bake".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: BAKE_SHADER,
            targets: vec![Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    };
    let display = cache.queue_render_pipeline(pipeline(DISPLAY_FORMAT));
    let light = cache.queue_render_pipeline(pipeline(LIGHT_FORMAT));
    commands.insert_resource(BakePipelines { layout, display, light });
}

fn extract_bake(bake: Extract<Option<Res<SkyBake>>>, mut job: ResMut<BakeJob>) {
    let Some(bake) = bake.as_deref() else { return };
    if bake.generation != bake.baked() && job.0.as_ref().is_none_or(|j| j.generation != bake.generation) {
        job.0 = Some(bake.clone());
    }
}

fn bake_sky(
    mut job: ResMut<BakeJob>,
    pipelines: Option<Res<BakePipelines>>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut ctx: RenderContext,
) {
    let (Some(bake), Some(pipelines)) = (job.0.as_ref(), pipelines) else {
        return;
    };
    let (Some(display), Some(light)) = (images.get(&bake.display), images.get(&bake.light)) else {
        return;
    };
    let (Some(display_pipeline), Some(light_pipeline)) =
        (cache.get_render_pipeline(pipelines.display), cache.get_render_pipeline(pipelines.light))
    else {
        return;
    };
    let mut sky = UniformBuffer::from(bake.sky);
    sky.write_buffer(&device, &queue);
    let layout = cache.get_bind_group_layout(&pipelines.layout);
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let targets = [
        (display, display_pipeline, 0, DISPLAY_FACE, "sky_bake_display"),
        (light, light_pipeline, 1, LIGHT_FACE, "sky_bake_light"),
    ];
    for (target, pipeline, is_light, size, label) in targets {
        for face in 0..6u32 {
            let mut uniform = UniformBuffer::from(FaceUniform {
                face,
                light: is_light,
                texel: 2.0 / size as f32,
                pad: 0.0,
            });
            uniform.write_buffer(&device, &queue);
            let (Some(sky_binding), Some(face_binding)) = (sky.binding(), uniform.binding()) else {
                return;
            };
            let bind_group = device.create_bind_group("sky_bake", &layout, &BindGroupEntries::sequential((sky_binding, face_binding)));
            let view = target.texture.create_view(&TextureViewDescriptor {
                label: Some("sky_bake_face"),
                dimension: Some(TextureViewDimension::D2),
                base_array_layer: face,
                array_layer_count: Some(1),
                ..default()
            });
            let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
                label: Some("sky_bake"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })],
                ..default()
            });
            let span = diagnostics.pass_span(&mut pass, label);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &*bind_group, &[]);
            pass.draw(0..3, 0..1);
            span.end(&mut pass);
        }
    }
    bake.baked.store(bake.generation, Ordering::Release);
    job.0 = None;
}
