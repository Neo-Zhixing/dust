use crate::device_info::DeviceInfo;

use ash::vk;
use bevy::{ecs::system::SystemState, prelude::*};

use std::io::Cursor;

pub struct RayShaders {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub sbt: super::sbt::Sbt,
    pub depth_sampler: vk::Sampler,
}

impl FromWorld for RayShaders {
    fn from_world(world: &mut World) -> Self {
        let (device, raytracing_loader, device_info, mut allocator, render_state) =
            SystemState::<(
                Res<ash::Device>,
                Res<ash::extensions::khr::RayTracingPipeline>,
                Res<DeviceInfo>,
                ResMut<crate::Allocator>,
                Res<crate::render::RenderState>,
            )>::new(world)
            .get_mut(world);

        unsafe {
            let depth_sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::builder()
                        .mag_filter(vk::Filter::NEAREST)
                        .min_filter(vk::Filter::NEAREST)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .build(),
                    None,
                )
                .unwrap();

            let pipeline_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::builder()
                        .set_layouts(&[render_state.per_window_desc_set_layout])
                        .build(),
                    None,
                )
                .unwrap();

            macro_rules! create_shader_module {
                ($name: literal) => {
                    device
                        .create_shader_module(
                            &vk::ShaderModuleCreateInfo::builder()
                                .flags(vk::ShaderModuleCreateFlags::empty())
                                .code(
                                    &ash::util::read_spv(&mut Cursor::new(
                                        &include_bytes!(concat!(
                                            env!("OUT_DIR"),
                                            "/shaders/",
                                            $name
                                        ))[..],
                                    ))
                                    .unwrap(),
                                )
                                .build(),
                            None,
                        )
                        .expect(concat!("Cannot build ", $name))
                };
            }

            let sbt_builder = super::sbt::SbtBuilder::new(
                create_shader_module!("raygen.rgen.spv"),
                vec![
                    create_shader_module!("miss.rmiss.spv"),
                    create_shader_module!("shadow.rmiss.spv"),
                ],
                [super::sbt::HitGroup {
                    ty: super::sbt::HitGroupType::Procedural,
                    intersection_shader: Some(create_shader_module!("esvo.rint.spv")),
                    anyhit_shader: None,
                    closest_hit_shader: Some(create_shader_module!("closest_hit.rchit.spv")),
                }]
                .iter(),
            );
            /*let deferred_operation = deferred_operation_loader
            .create_deferred_operation(None)
            .unwrap(); */
            let raytracing_pipeline = sbt_builder
                .create_raytracing_pipeline(&*raytracing_loader, &*device, pipeline_layout, 2)
                .unwrap();
            let sbt = sbt_builder.create_sbt(
                &*raytracing_loader,
                &*device,
                &mut *allocator,
                raytracing_pipeline,
                &device_info.raytracing_pipeline_properties,
            );
            drop(sbt_builder);
            RayShaders {
                pipeline: raytracing_pipeline,
                pipeline_layout,
                sbt,
                depth_sampler,
            }
        }
    }
}
