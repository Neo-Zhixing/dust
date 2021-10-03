//! Utils for Shader Binding Table


use ash::{prelude::VkResult, vk};

#[derive(Clone, Copy)]
pub enum HitGroupType {
    Triangles,
    Procedural,
}
pub struct HitGroup {
    pub ty: HitGroupType,
    pub intersection_shader: Option<vk::ShaderModule>,
    pub anyhit_shader: Option<vk::ShaderModule>,
    pub closest_hit_shader: Option<vk::ShaderModule>,
}

struct HitGroupEntry {
    ty: HitGroupType,
    intersection_shader: Option<u32>,
    anyhit_shader: Option<u32>,
    closest_hit_shader: Option<u32>,
}

pub struct SbtBuilder {
    raygen_shader: vk::ShaderModule,
    miss_shaders: Vec<vk::ShaderModule>,
    hitgroup_shaders: Vec<(vk::ShaderStageFlags, vk::ShaderModule)>,
    hitgroups: Vec<HitGroupEntry>,
}

pub struct Sbt {
    pub memory: gpu_alloc::MemoryBlock<vk::DeviceMemory>,
    pub buffer: vk::Buffer,
    pub raygen_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
    pub hit_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
    pub miss_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
    pub callable_shader_binding_tables: vk::StridedDeviceAddressRegionKHR,
}

impl SbtBuilder {
    pub fn new<'a>(
        raygen_shader: vk::ShaderModule,
        miss_shaders: Vec<vk::ShaderModule>,
        hitgroups: impl ExactSizeIterator<Item = &'a HitGroup>,
    ) -> Self {
        let mut hitgroup_shaders: Vec<(vk::ShaderStageFlags, vk::ShaderModule)> =
            Vec::with_capacity(hitgroups.len() * 3);
        let hitgroup_entries: Vec<HitGroupEntry> = hitgroups
            .map(|hitgroup| {
                let mut entry = HitGroupEntry {
                    ty: hitgroup.ty,
                    intersection_shader: None,
                    anyhit_shader: None,
                    closest_hit_shader: None,
                };
                macro_rules! push_shader {
                    ($shader_type: ident, $shader_stage_flags: expr) => {
                        if let Some(hitgroup_shader) = hitgroup.$shader_type {
                            let index = hitgroup_shaders
                                .iter()
                                .position(|(_, shader)| *shader == hitgroup_shader)
                                .unwrap_or_else(|| {
                                    let index = hitgroup_shaders.len();
                                    hitgroup_shaders.push(($shader_stage_flags, hitgroup_shader));
                                    index
                                });
                            entry.$shader_type = Some(index as u32);
                        }
                    };
                }
                push_shader!(intersection_shader, vk::ShaderStageFlags::INTERSECTION_KHR);
                push_shader!(anyhit_shader, vk::ShaderStageFlags::ANY_HIT_KHR);
                push_shader!(closest_hit_shader, vk::ShaderStageFlags::CLOSEST_HIT_KHR);
                entry
            })
            .collect();
        Self {
            raygen_shader,
            miss_shaders,
            hitgroup_shaders,
            hitgroups: hitgroup_entries,
        }
    }
    pub unsafe fn create_raytracing_pipeline(
        &self,
        raytracing_loader: &ash::extensions::khr::RayTracingPipeline,
        device: &ash::Device,
        layout: vk::PipelineLayout,
        max_recursion_depth: u32,
    ) -> VkResult<vk::Pipeline> {
        fn create_stage(
            module: vk::ShaderModule,
            stage: vk::ShaderStageFlags,
            specialization_info: &vk::SpecializationInfo,
        ) -> vk::PipelineShaderStageCreateInfo {
            const SHADER_ENTRY_NAME_MAIN: &'static std::ffi::CStr =
                unsafe { std::ffi::CStr::from_bytes_with_nul_unchecked(b"main\0") };
            vk::PipelineShaderStageCreateInfo::builder()
                .flags(vk::PipelineShaderStageCreateFlags::default())
                .stage(stage)
                .module(module)
                .name(SHADER_ENTRY_NAME_MAIN)
                .specialization_info(specialization_info)
                .build()
        }
        let default_specialization_info = vk::SpecializationInfo::default();

        let stages: Vec<vk::PipelineShaderStageCreateInfo> = std::iter::once(create_stage(
            self.raygen_shader,
            vk::ShaderStageFlags::RAYGEN_KHR,
            &default_specialization_info,
        ))
        .chain(self.miss_shaders.iter().map(|&module| {
            create_stage(
                module,
                vk::ShaderStageFlags::MISS_KHR,
                &default_specialization_info,
            )
        }))
        .chain(
            self.hitgroup_shaders
                .iter()
                .map(|(stage, module)| create_stage(*module, *stage, &default_specialization_info)),
        )
        .collect();

        let groups: Vec<vk::RayTracingShaderGroupCreateInfoKHR> = std::iter::once(
            vk::RayTracingShaderGroupCreateInfoKHR::builder() // Raygen Shader
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(0) // Raygen shader is always at num 0
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .build(),
        )
        .chain((1..=self.miss_shaders.len()).map(|i| {
            vk::RayTracingShaderGroupCreateInfoKHR::builder() // Miss Shader
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(i as u32)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .build()
        }))
        .chain(self.hitgroups.iter().map(|group| {
            let base = self.miss_shaders.len() as u32 + 1;
            let ty = match group.ty {
                HitGroupType::Procedural => vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP,
                HitGroupType::Triangles => vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP,
            };
            vk::RayTracingShaderGroupCreateInfoKHR::builder() // Miss Shader
                .ty(ty)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(
                    group
                        .intersection_shader
                        .map_or(vk::SHADER_UNUSED_KHR, |i| i + base),
                )
                .any_hit_shader(
                    group
                        .anyhit_shader
                        .map_or(vk::SHADER_UNUSED_KHR, |i| i + base),
                )
                .closest_hit_shader(
                    group
                        .closest_hit_shader
                        .map_or(vk::SHADER_UNUSED_KHR, |i| i + base),
                )
                .build()
        }))
        .collect();

        let mut raytracing_pipeline = vk::Pipeline::null();
        let result = raytracing_loader.fp().create_ray_tracing_pipelines_khr(
            device.handle(),
            vk::DeferredOperationKHR::null(),
            vk::PipelineCache::null(),
            1,
            [vk::RayTracingPipelineCreateInfoKHR::builder()
                .flags(vk::PipelineCreateFlags::default())
                .stages(&stages)
                .groups(&groups)
                .max_pipeline_ray_recursion_depth(max_recursion_depth)
                .layout(layout) // TODO
                .build()]
            .as_ptr(),
            std::ptr::null(),
            &mut raytracing_pipeline,
        );
        match result {
            vk::Result::SUCCESS => Ok(raytracing_pipeline),
            _ => Err(result),
        }
    }

    pub unsafe fn create_sbt(
        &self,
        raytracing_loader: &ash::extensions::khr::RayTracingPipeline,
        device: &ash::Device,
        allocator: &mut crate::Allocator,
        pipeline: vk::Pipeline,
        raytracing_pipeline_properties: &vk::PhysicalDeviceRayTracingPipelinePropertiesKHR,
    ) -> Sbt {
        use gpu_alloc::Request;
        use gpu_alloc_ash::AshMemoryDevice;
        use std::alloc::Layout;

        let total_num_groups = 1 + self.miss_shaders.len() as u32 + self.hitgroups.len() as u32;

        let handle_layout = Layout::from_size_align(
            raytracing_pipeline_properties.shader_group_handle_size as usize,
            raytracing_pipeline_properties.shader_group_handle_alignment as usize,
        )
        .unwrap();

        let raygen_layout = handle_layout
            .align_to(raytracing_pipeline_properties.shader_group_base_alignment as usize)
            .unwrap();
        let raymiss_layout = handle_layout
            .repeat(self.miss_shaders.len())
            .unwrap()
            .0
            .align_to(raytracing_pipeline_properties.shader_group_base_alignment as usize)
            .unwrap();
        let hitgroup_layout = handle_layout
            .repeat(self.hitgroups.len())
            .unwrap()
            .0
            .align_to(raytracing_pipeline_properties.shader_group_base_alignment as usize)
            .unwrap();
        let sbt_layout = raygen_layout
            .extend(raymiss_layout)
            .unwrap()
            .0
            .extend(hitgroup_layout)
            .unwrap()
            .0;
        assert_eq!(
            sbt_layout.align(),
            raytracing_pipeline_properties.shader_group_base_alignment as usize
        );

        let sbt_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .size(sbt_layout.size() as u64)
                    .usage(
                        vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR
                            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS_KHR,
                    )
                    .build(),
                None,
            )
            .unwrap();
        let requirements = device.get_buffer_memory_requirements(sbt_buf);
        let mut sbt_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&*device),
                Request {
                    size: requirements.size,
                    align_mask: num::integer::lcm(
                        requirements.alignment,
                        sbt_layout.align() as u64,
                    ),
                    usage: gpu_alloc::UsageFlags::UPLOAD,
                    memory_types: requirements.memory_type_bits,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(sbt_buf, *sbt_mem.memory(), sbt_mem.offset())
            .unwrap();
        let device_address = device.get_buffer_device_address(
            &vk::BufferDeviceAddressInfo::builder()
                .buffer(sbt_buf)
                .build(),
        );
        assert_eq!(device_address % sbt_layout.align() as u64, 0);
        {
            let sbt_handles_host_vec = raytracing_loader
                .get_ray_tracing_shader_group_handles(
                    pipeline,
                    0,
                    total_num_groups,
                    raytracing_pipeline_properties.shader_group_handle_size as usize
                        * total_num_groups as usize,
                    // VUID-vkGetRayTracingShaderGroupHandlesKHR-dataSize-02420
                    // dataSize must be at least VkPhysicalDeviceRayTracingPipelinePropertiesKHR::shaderGroupHandleSize × groupCount
                )
                .unwrap();
            let mut sbt_handles_host = sbt_handles_host_vec.as_ptr();
            let mut sbt_handles_device = sbt_mem
                .map(AshMemoryDevice::wrap(&*device), 0, sbt_layout.size())
                .unwrap()
                .as_ptr();

            // Copy Raygen
            std::ptr::copy_nonoverlapping(
                sbt_handles_host,
                sbt_handles_device,
                handle_layout.size(),
            );
            sbt_handles_host = sbt_handles_host.add(handle_layout.size());
            sbt_handles_device = sbt_handles_device.add(raygen_layout.pad_to_align().size());

            {
                // Copy RayMiss
                let mut current_device = sbt_handles_device;
                for _ in 0..self.miss_shaders.len() {
                    std::ptr::copy_nonoverlapping(
                        sbt_handles_host,
                        current_device,
                        handle_layout.size(),
                    );
                    sbt_handles_host = sbt_handles_host.add(handle_layout.size());
                    current_device = current_device.add(handle_layout.pad_to_align().size());
                }
                sbt_handles_device = sbt_handles_device.add(raymiss_layout.pad_to_align().size());
            }
            {
                // Copy Hitgroups
                let mut current_device = sbt_handles_device;
                for _ in 0..self.hitgroups.len() {
                    std::ptr::copy_nonoverlapping(
                        sbt_handles_host,
                        sbt_handles_device,
                        handle_layout.size(),
                    );
                    sbt_handles_host = sbt_handles_host.add(handle_layout.size());
                    current_device = current_device.add(handle_layout.pad_to_align().size());
                }
            }

            sbt_mem.unmap(AshMemoryDevice::wrap(&*device));
        }
        Sbt {
            memory: sbt_mem,
            buffer: sbt_buf,
            raygen_shader_binding_tables: vk::StridedDeviceAddressRegionKHR {
                device_address: device_address,
                size: raygen_layout.size() as u64,
                stride: handle_layout.pad_to_align().size() as u64,
            },
            miss_shader_binding_tables: vk::StridedDeviceAddressRegionKHR {
                device_address: device_address + raygen_layout.pad_to_align().size() as u64,
                size: raymiss_layout.size() as u64,
                stride: handle_layout.pad_to_align().size() as u64,
            },
            hit_shader_binding_tables: vk::StridedDeviceAddressRegionKHR {
                device_address: device_address
                    + raygen_layout.pad_to_align().size() as u64
                    + raymiss_layout.pad_to_align().size() as u64,
                size: hitgroup_layout.size() as u64,
                stride: handle_layout.pad_to_align().size() as u64,
            },
            callable_shader_binding_tables: vk::StridedDeviceAddressRegionKHR::default(),
        }
    }
}
