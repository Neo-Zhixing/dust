use bevy::prelude::*;
use ash::vk;
use std::{ffi::CStr, io::Cursor};

use crate::device_info::DeviceInfo;

pub(super) fn raytracing_setup(
    device: Res<ash::Device>,
    raytracing_loader: Res<ash::extensions::khr::RayTracingPipeline>,
    device_info: Res<DeviceInfo>
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
        let mut raytracing_pipeline = vk::Pipeline::null();
        let result = raytracing_loader
        .fp()
        .create_ray_tracing_pipelines_khr(
            device.handle(),
            vk::DeferredOperationKHR::null(),
            vk::PipelineCache::null(),
            1,
            [
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
                            .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                            .general_shader(0)
                            .intersection_shader(vk::SHADER_UNUSED_KHR)
                            .any_hit_shader(vk::SHADER_UNUSED_KHR)
                            .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                            .build(),
                        vk::RayTracingShaderGroupCreateInfoKHR::builder()
                            .ty(vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP)
                            .intersection_shader(1)
                            .any_hit_shader(vk::SHADER_UNUSED_KHR)
                            .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                            .build(),
                    ])
                    .max_pipeline_ray_recursion_depth(device_info.raytracing_pipeline_properties.max_ray_recursion_depth.max(1))
                    .layout(pipeline_layout) // TODO
                    .build()
            ].as_ptr(),
            std::ptr::null(),
            &mut raytracing_pipeline
        );
        assert_eq!(result, vk::Result::SUCCESS);

        
        let layout = std::alloc::Layout::from_size_align(
            device_info.raytracing_pipeline_properties.shader_group_handle_size as usize,
            device_info.raytracing_pipeline_properties.shader_group_handle_alignment as usize
        ).unwrap();
        let (layout, _) = layout.repeat(2).unwrap(); // group size

        let handles = raytracing_loader.get_ray_tracing_shader_group_handles(
            raytracing_pipeline,
            0,
            2, 
            layout.size(),
        ).unwrap();
        
        //println!("{:?}", raytracing_pipelie);
    }

}
