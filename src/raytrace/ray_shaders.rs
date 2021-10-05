use bevy::{ecs::system::SystemState, prelude::*};
use gpu_alloc::Request;
use gpu_alloc_ash::AshMemoryDevice;

use super::block_alloc::BlockAllocator;
use super::tlas::TlasState;
use crate::device_info::DeviceInfo;
use crate::raytrace::RaytracingNodeViewConstants;
use ash::vk;
use std::ffi::CStr;
use std::io::Cursor;
use std::sync::Arc;

pub struct RayShaders {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub target_img_desc_layout: vk::DescriptorSetLayout,
    pub target_img_desc_set: vk::DescriptorSet,
    pub sbt: super::sbt::Sbt,
    pub depth_sampler: vk::Sampler,
}

impl FromWorld for RayShaders {
    fn from_world(world: &mut World) -> Self {
        let (
            tlas_state,
            block_allocator,
            device,
            raytracing_loader,
            device_info,
            mut allocator,
            desc_pool,
        ) = SystemState::<(
            Res<TlasState>,
            Res<Arc<dyn BlockAllocator>>,
            Res<ash::Device>,
            Res<ash::extensions::khr::RayTracingPipeline>,
            Res<DeviceInfo>,
            ResMut<crate::Allocator>,
            Res<vk::DescriptorPool>,
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
            let target_img_desc_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::builder()
                        .bindings(&[
                            vk::DescriptorSetLayoutBinding::builder()
                                .binding(0)
                                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                                .descriptor_count(1)
                                .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
                                .build(),
                            vk::DescriptorSetLayoutBinding::builder()
                                .binding(1)
                                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                                .descriptor_count(1)
                                .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
                                .build(),
                            vk::DescriptorSetLayoutBinding::builder()
                                .binding(2)
                                .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                                .descriptor_count(1)
                                .stage_flags(
                                    vk::ShaderStageFlags::RAYGEN_KHR
                                        | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
                                )
                                .build(),
                            vk::DescriptorSetLayoutBinding::builder()
                                .binding(3)
                                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                                .descriptor_count(1)
                                .stage_flags(vk::ShaderStageFlags::INTERSECTION_KHR)
                                .build(),
                        ])
                        .build(),
                    None,
                )
                .unwrap();
            let mut target_img_desc_set = vk::DescriptorSet::null();
            let result = device.fp_v1_0().allocate_descriptor_sets(
                device.handle(),
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(*desc_pool)
                    .set_layouts(&[target_img_desc_layout])
                    .build(),
                &mut target_img_desc_set,
            );
            assert_eq!(result, vk::Result::SUCCESS);

            let pipeline_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::builder()
                        .set_layouts(&[target_img_desc_layout])
                        .push_constant_ranges(&[vk::PushConstantRange {
                            stage_flags: vk::ShaderStageFlags::RAYGEN_KHR,
                            offset: 0,
                            size: std::mem::size_of::<RaytracingNodeViewConstants>() as u32,
                        }])
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

            /*
            TODO
            device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::builder()
                    .dst_set(target_img_desc_set)
                    .dst_binding(3)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&[vk::DescriptorBufferInfo::builder()
                        .buffer(block_allocator.get_buffer())
                        .offset(0)
                        .range(block_allocator.get_device_buffer_size())
                        .build()])
                    .build()],
                &[],
            );
            */
            RayShaders {
                pipeline: raytracing_pipeline,
                pipeline_layout,
                target_img_desc_layout,
                target_img_desc_set,
                sbt,
                depth_sampler,
            }
        }
    }
}
