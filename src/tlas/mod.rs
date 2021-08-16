struct RaytracedEntity;
use bevy::prelude::*;
use ash::vk;

fn tlas_setup(
    device: Res<ash::Device>,
    mut allocator: ResMut<crate::Allocator>,
    acceleration_structure_loader: Res<ash::extensions::khr::AccelerationStructure>,
    deferred_host_operations_loader: Res<ash::extensions::khr::DeferredHostOperations>,
) {
    unsafe {
        let deferred_operation = deferred_host_operations_loader.create_deferred_operation(None).unwrap();
        let simple_box = acceleration_structure_loader.create_acceleration_structure(
            &vk::AccelerationStructureCreateInfoKHR::builder()
            .buffer()
            .offset()
            .size()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .device_address()
            .build(),
            None
        );
        acceleration_structure_loader.build_acceleration_structures(
            deferred_operation,
            &[
                vk::AccelerationStructureBuildGeometryInfoKHR::builder()
                    .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
                    .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
                    .build()
            ],
            &[
                &[
                    vk::AccelerationStructureBuildRangeInfoKHR::builder()
                    .build()
                ]
            ],
        ).unwrap();
    }
}
