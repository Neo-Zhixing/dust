use bevy::prelude::*;
use ash::vk;
use std::{ffi::CStr, io::Cursor};

pub(super) fn raytracing_setup(
    device: Res<ash::Device>,
    raytracing_loader: Res<ash::extensions::khr::RayTracingPipeline>,
    deferred_operation_loader: Res<ash::extensions::khr::DeferredHostOperations>,
) {
    unsafe {
        let pipeline_layout = device.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&[

            ])
            .build(), None)
            .unwrap();
        let raygen_shader_module = device.create_shader_module(
            &vk::ShaderModuleCreateInfo::builder()
                .flags(vk::ShaderModuleCreateFlags::empty())
                .code(&ash::util::read_spv(&mut Cursor::new(
                    &include_bytes!(concat!(env!("OUT_DIR"), "/shaders/raygen.rgen.spv"))[..],
                ))
                .unwrap(),)
                .build(), None)
                .expect("Cannot build raygen shader");

        let intersection_shader_module = device.create_shader_module(
            &vk::ShaderModuleCreateInfo::builder()
                .flags(vk::ShaderModuleCreateFlags::empty())
                .code(&ash::util::read_spv(&mut Cursor::new(
                    &include_bytes!(concat!(env!("OUT_DIR"), "/shaders/esvo.rint.spv"))[..],
                ))
                .unwrap(),)
                .build(), None)
                .expect("Cannot build intersection shader");
        /*let deferred_operation = deferred_operation_loader
        .create_deferred_operation(None)
        .unwrap(); */
        let raytracing_pipelie = raytracing_loader.create_ray_tracing_pipelines(
            vk::DeferredOperationKHR::null(),
            vk::PipelineCache::null(),
            &[
                vk::RayTracingPipelineCreateInfoKHR::builder()
                    .flags(vk::PipelineCreateFlags::default())
                    .stages(&[
                        vk::PipelineShaderStageCreateInfo::builder()
                            .flags(vk::PipelineShaderStageCreateFlags::default())
                            .stage(vk::ShaderStageFlags::RAYGEN_KHR)
                            .module(raygen_shader_module)
                            .name(CStr::from_bytes_with_nul_unchecked(b"main\0"))
                            .specialization_info(&vk::SpecializationInfo::builder()
                                .build())
                            .build(),
                        vk::PipelineShaderStageCreateInfo::builder()
                        .flags(vk::PipelineShaderStageCreateFlags::default())
                        .stage(vk::ShaderStageFlags::INTERSECTION_KHR)
                        .module(intersection_shader_module)
                        .name(CStr::from_bytes_with_nul_unchecked(b"main\0"))
                        .specialization_info(&vk::SpecializationInfo::builder()
                            .build())
                        .build()
                    ])
                    .groups(&[
                        vk::RayTracingShaderGroupCreateInfoKHR::builder()
                            .ty(vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP)
                            .intersection_shader(1)
                            .any_hit_shader(vk::SHADER_UNUSED_KHR)
                            .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                            .build() // TODO
                    ])
                    .max_pipeline_ray_recursion_depth(3)
                    .library_info(&vk::PipelineLibraryCreateInfoKHR::builder()
                        .build()) // TOD
                    .library_interface(&vk::RayTracingPipelineInterfaceCreateInfoKHR::builder()
                        .max_pipeline_ray_payload_size(4)
                        .max_pipeline_ray_hit_attribute_size(4)
                        .build()) // TODO
                    .layout(pipeline_layout) // TODO
                    .build()
            ],
            None
        )
        .unwrap();
        println!("{:?}", raytracing_pipelie);
    }

}
