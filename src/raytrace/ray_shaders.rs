use bevy::{ecs::system::SystemState, prelude::*};
use crate::device_info::DeviceInfo;
use crate::raytrace::RaytracingNodeViewConstants;
use ash::vk;

use std::io::Cursor;

pub struct RayShaders {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub raytracing_resources_desc_layout: vk::DescriptorSetLayout,
    pub raytracing_resources_desc_set: vk::DescriptorSet,
    pub sbt: super::sbt::Sbt,
    pub depth_sampler: vk::Sampler,
    pub desc_pool: vk::DescriptorPool,
}

impl FromWorld for RayShaders {
    fn from_world(world: &mut World) -> Self {
        let (
            device,
            raytracing_loader,
            device_info,
            mut allocator,
            render_state,
        ) = SystemState::<(
            Res<ash::Device>,
            Res<ash::extensions::khr::RayTracingPipeline>,
            Res<DeviceInfo>,
            ResMut<crate::Allocator>,
            Res<crate::render::RenderState>
        )>::new(world)
        .get_mut(world);

        unsafe {
            let desc_pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::builder()
                .flags(vk::DescriptorPoolCreateFlags::empty())
                .max_sets(2)
                .pool_sizes(&[
                    vk::DescriptorPoolSize {
                        ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                        descriptor_count: 1,
                    },
                    vk::DescriptorPoolSize {
                        ty: vk::DescriptorType::STORAGE_BUFFER,
                        descriptor_count: 1,
                    }
                ])
                .build(),
                None,
            ).unwrap();
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
            let raytracing_resources_desc_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::builder()
                        .bindings(&[
                            vk::DescriptorSetLayoutBinding::builder()
                                .binding(0)
                                .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                                .descriptor_count(1)
                                .stage_flags(
                                    vk::ShaderStageFlags::RAYGEN_KHR
                                        | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
                                )
                                .build(), // Acceleration Structure
                            vk::DescriptorSetLayoutBinding::builder()
                                .binding(1)
                                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                                .descriptor_count(1)
                                .stage_flags(vk::ShaderStageFlags::INTERSECTION_KHR)
                                .build(), // Octree Nodes
                        ])
                        .build(),
                    None,
                )
                .unwrap();
            let mut raytracing_resources_desc_set = vk::DescriptorSet::null();
            let result = device.fp_v1_0().allocate_descriptor_sets(
                device.handle(),
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[raytracing_resources_desc_layout])
                    .build(),
                &mut raytracing_resources_desc_set,
            );
            assert_eq!(result, vk::Result::SUCCESS);

            let pipeline_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::builder()
                        .set_layouts(&[
                            render_state.per_window_desc_set_layout,
                            raytracing_resources_desc_layout
                        ])
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
                raytracing_resources_desc_layout,
                raytracing_resources_desc_set,
                desc_pool,
            }
        }
    }
}
