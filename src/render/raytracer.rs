use bevy::prelude::*;
use ash::{extensions::khr::RayTracingPipeline, vk};
use gpu_alloc::Request;
use gpu_alloc_ash::AshMemoryDevice;
use std::{ffi::CStr, io::Cursor};

use crate::{device_info::DeviceInfo, render::commands};

pub(crate) struct RaytracingPipelineState {
    pub pipeline: vk::Pipeline,
    sbt_mem: gpu_alloc::MemoryBlock<vk::DeviceMemory>,
    sbt_buf: vk::Buffer,
    pub raygen_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
    pub miss_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
    pub hit_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
    pub callable_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
}

pub(super) fn raytracing_setup(
    mut commands: Commands,
    device: Res<ash::Device>,
    raytracing_loader: Res<ash::extensions::khr::RayTracingPipeline>,
    device_info: Res<DeviceInfo>,
    mut allocator: ResMut<crate::Allocator>,
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

        
        let sbt_entry_layout = std::alloc::Layout::from_size_align(
            device_info.raytracing_pipeline_properties.shader_group_handle_size as usize,
            device_info.raytracing_pipeline_properties.shader_group_handle_alignment as usize
        ).unwrap();
        let sbt_group_layout = sbt_entry_layout.align_to(device_info.raytracing_pipeline_properties.shader_group_base_alignment as usize).unwrap();

        let (sbt_layout, _) = sbt_group_layout.repeat(2).unwrap(); // group size

        let sbt_handles_host = raytracing_loader.get_ray_tracing_shader_group_handles(
            raytracing_pipeline,
            0,
            2, 
            device_info.raytracing_pipeline_properties.shader_group_handle_size as usize * 2,
        ).unwrap();
        // Now, copy the sbt to device memory
        let sbt_buf = device.create_buffer(
            &vk::BufferCreateInfo::builder()
            .size(sbt_layout.size() as u64 + device_info.raytracing_pipeline_properties.shader_group_base_alignment as u64)
            .usage(vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS_KHR)
            .build(),
            None
        ).unwrap();
        let requirements = device.get_buffer_memory_requirements(sbt_buf);
        
        // now, copy the sbt to vram
        let mut sbt_mem = allocator.alloc(AshMemoryDevice::wrap(&*device), Request {
            size: requirements.size,
            align_mask: requirements.alignment,
            usage: gpu_alloc::UsageFlags::UPLOAD,
            memory_types: requirements.memory_type_bits
        }).unwrap();
        device.bind_buffer_memory(sbt_buf, *sbt_mem.memory(), sbt_mem.offset()).unwrap();



        let device_address = device.get_buffer_device_address(&vk::BufferDeviceAddressInfo::builder()
            .buffer(sbt_buf)
            .build()
        );
        let rounded_device_address = crate::util::round_up(device_address, device_info.raytracing_pipeline_properties.shader_group_base_alignment as u64);

        let sbt_handles_device = sbt_mem.map(AshMemoryDevice::wrap(&*device),  rounded_device_address - device_address, sbt_layout.size()).unwrap();


        {
            // copy the sbt to vram
            let mut host_ptr = sbt_handles_host.as_ptr();
            let mut device_ptr = sbt_handles_device.as_ptr();
            for _ in 0..2 {
                std::ptr::copy_nonoverlapping(host_ptr, device_ptr, device_info.raytracing_pipeline_properties.shader_group_handle_size as usize);
                host_ptr = host_ptr.add(device_info.raytracing_pipeline_properties.shader_group_handle_size as usize);
                device_ptr = device_ptr.add(sbt_group_layout.pad_to_align().size());
            }
        }

        
        sbt_mem.unmap(AshMemoryDevice::wrap(&*device));
        let device_address = rounded_device_address;


        let state = RaytracingPipelineState {
            pipeline: raytracing_pipeline,
            sbt_buf,
            sbt_mem,
            raygen_shader_binding_tables: vk::StridedDeviceAddressRegionKHR {
                device_address,
                stride: sbt_entry_layout.pad_to_align().size() as u64,
                size: sbt_entry_layout.pad_to_align().size() as u64,
            },
            miss_shader_binding_tables: vk::StridedDeviceAddressRegionKHR::default(),
            hit_shader_binding_tables: vk::StridedDeviceAddressRegionKHR {
                device_address: device_address + sbt_group_layout.pad_to_align().size() as u64,
                stride: sbt_entry_layout.pad_to_align().size() as u64,
                size: sbt_entry_layout.pad_to_align().size() as u64,
            },
            callable_shader_binding_tables: vk::StridedDeviceAddressRegionKHR::default(),
        };
        commands.insert_resource(state);
    }
}
