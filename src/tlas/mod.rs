struct RaytracedEntity;
use ash::vk;
use bevy::prelude::*;
use gpu_alloc_ash::AshMemoryDevice;

use crate::{Queues, device_info::DeviceInfo};

#[derive(Default)]
pub struct TlasPlugin;

impl Plugin for TlasPlugin {
    fn build(&self, app: &mut App) {
        app.add_startup_system_to_stage(StartupStage::Startup, tlas_setup);
    }
}

struct TlasState {
    unit_box_as: vk::AccelerationStructureKHR,
    unit_box_as_buf: vk::Buffer,
    unit_box_as_mem: crate::MemoryBlock,
}

fn tlas_setup(
    mut commands: Commands,
    device: Res<ash::Device>,
    mut allocator: ResMut<crate::Allocator>,
    queues: Res<Queues>,
    device_info: Res<DeviceInfo>,
    acceleration_structure_loader: Res<ash::extensions::khr::AccelerationStructure>,
) {
    unsafe {
        let command_pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::builder()
                    .flags(vk::CommandPoolCreateFlags::TRANSIENT)
                    .queue_family_index(queues.compute_queue_family)
                    .build(),
                None,
            )
            .unwrap();
        let command_buffer = {
            let mut command_buffer = vk::CommandBuffer::null();
            let result = device.fp_v1_0().allocate_command_buffers(
                device.handle(),
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1)
                    .build() as *const _,
                &mut command_buffer,
            );
            assert_eq!(result, vk::Result::SUCCESS);
            command_buffer
        };

        let unit_box = vk::AabbPositionsKHR {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 1.0,
            max_y: 1.0,
            max_z: 1.0,
        };
        let unit_box_buffer = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .size(std::mem::size_of_val(&unit_box) as u64)
                    .usage(vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let unit_box_requirements = device.get_buffer_memory_requirements(unit_box_buffer);
        let mut unit_box_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: unit_box_requirements.size,
                    align_mask: unit_box_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::DEVICE_ADDRESS
                        | gpu_alloc::UsageFlags::TRANSIENT
                        | gpu_alloc::UsageFlags::UPLOAD,
                    memory_types: unit_box_requirements.memory_type_bits,
                },
            )
            .unwrap();
        unit_box_mem.write_bytes(
            AshMemoryDevice::wrap(&device),
            0,
            std::slice::from_raw_parts(
                &unit_box as *const _ as *const u8,
                std::mem::size_of_val(&unit_box),
            ),
        ).unwrap();
        device
            .bind_buffer_memory(
                unit_box_buffer,
                *unit_box_mem.memory(),
                unit_box_mem.offset(),
            )
            .unwrap();
        let unit_box_device_address = device.get_buffer_device_address(
            &vk::BufferDeviceAddressInfo::builder()
            .buffer(unit_box_buffer)
            .build()
        );
        let mut build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .geometries(&[vk::AccelerationStructureGeometryKHR::builder()
                .geometry_type(vk::GeometryTypeKHR::AABBS)
                .flags(vk::GeometryFlagsKHR::default())
                .geometry(vk::AccelerationStructureGeometryDataKHR {
                    aabbs: vk::AccelerationStructureGeometryAabbsDataKHR::builder()
                        .data(vk::DeviceOrHostAddressConstKHR {
                            device_address: unit_box_device_address
                        })
                        .stride(std::mem::size_of_val(&unit_box) as u64)
                        .build(),
                })
                .build()])
            .build();
        let sizes = acceleration_structure_loader.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::HOST,
            &build_geometry_info,
            &[1],
        );
        
        let unit_box_as_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .flags(vk::BufferCreateFlags::default())
                    .size(sizes.acceleration_structure_size)
                    .usage(vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let unit_box_as_buf_requirements = device.get_buffer_memory_requirements(unit_box_as_buf);
        let unit_box_as_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: unit_box_as_buf_requirements.size,
                    align_mask: unit_box_as_buf_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::FAST_DEVICE_ACCESS,
                    memory_types: unit_box_as_buf_requirements.memory_type_bits,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(
                unit_box_as_buf,
                *unit_box_as_mem.memory(),
                unit_box_as_mem.offset(),
            )
            .unwrap();

        
        let scratch_alignment = device_info.acceleration_structure_properties.min_acceleration_structure_scratch_offset_alignment as u64;
        let scratch_buf = device
        .create_buffer(
            &vk::BufferCreateInfo::builder()
                .flags(vk::BufferCreateFlags::default())
                .size(sizes.build_scratch_size + scratch_alignment)
                .usage(vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .build(),
            None,
        )
        .unwrap();
        let scratch_requirements = device.get_buffer_memory_requirements(scratch_buf);

        let scratch_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: scratch_requirements.size,
                    align_mask: scratch_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::FAST_DEVICE_ACCESS
                        | gpu_alloc::UsageFlags::TRANSIENT
                        | gpu_alloc::UsageFlags::DEVICE_ADDRESS,
                    memory_types: scratch_requirements.memory_type_bits,
                },
            )
            .unwrap();
        device.bind_buffer_memory(scratch_buf, *scratch_mem.memory(), scratch_mem.offset()).unwrap();
        let scratch_device_address = device.get_buffer_device_address(
            &vk::BufferDeviceAddressInfo::builder()
                .buffer(scratch_buf)
                .build(),
        );
        // Round up
        let scratch_device_address = ((scratch_device_address + scratch_alignment - 1) / scratch_alignment) * scratch_alignment;
        let unit_box_as = acceleration_structure_loader
            .create_acceleration_structure(
                &vk::AccelerationStructureCreateInfoKHR::builder()
                    .buffer(unit_box_as_buf)
                    .offset(0)
                    .size(sizes.acceleration_structure_size)
                    .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
                    // .device_address()
                    .build(),
                None,
            )
            .unwrap();
        build_geometry_info.dst_acceleration_structure = unit_box_as;
        build_geometry_info.scratch_data.device_address = scratch_device_address;

        device
            .begin_command_buffer(
                command_buffer,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                    .build(),
            )
            .unwrap();
        acceleration_structure_loader.cmd_build_acceleration_structures(
            command_buffer,
            &[build_geometry_info],
            &[&[vk::AccelerationStructureBuildRangeInfoKHR {
                primitive_count: 1,
                primitive_offset: 0,
                first_vertex: 0,
                transform_offset: 0,
            }]],
        );
        device.end_command_buffer(command_buffer).unwrap();
        device
            .queue_submit(
                queues.compute_queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[command_buffer])
                    .build()],
                vk::Fence::null(),
            )
            .unwrap();
        
        
        // Free memory
        device.destroy_buffer(scratch_buf, None);
        allocator.dealloc(AshMemoryDevice::wrap(&device), scratch_mem);
        device.destroy_buffer(unit_box_buffer, None);
        allocator.dealloc(AshMemoryDevice::wrap(&device), unit_box_mem);
        let tlas_state = TlasState {
            unit_box_as,
            unit_box_as_buf,
            unit_box_as_mem,
        };
        commands.insert_resource(tlas_state);
    }
}

