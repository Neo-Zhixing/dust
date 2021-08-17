#![feature(maybe_uninit_uninit_array)]

mod device_info;
mod queues;
mod render;
mod tlas;

use device_info::DeviceInfo;

use bevy::prelude::*;

use ash::vk;
use bevy::window::WindowCreated;
use bevy::winit::WinitWindows;
use std::borrow::Cow;
use std::ffi::CStr;

pub type Allocator = gpu_alloc::GpuAllocator<vk::DeviceMemory>;
pub type MemoryBlock = gpu_alloc::MemoryBlock<vk::DeviceMemory>;
pub use queues::Queues;
pub use tlas::TlasAABB;

#[derive(Default)]
pub struct DustPlugin;

impl Plugin for DustPlugin {
    fn build(&self, app: &mut App) {
        app.add_startup_system_to_stage(StartupStage::PreStartup, setup)
            .add_plugin(render::RenderPlugin::default())
            .add_plugin(tlas::TlasPlugin::default());
    }
}

fn setup(
    mut commands: Commands,
    mut window_created_events: EventReader<WindowCreated>,
    winit_windows: Res<WinitWindows>,
) {
    let window_id = window_created_events
        .iter()
        .next()
        .map(|event| event.id)
        .unwrap();

    let winit_window = winit_windows.get_window(window_id).unwrap();

    unsafe {
        let entry = ash::Entry::new().unwrap();
        let instance = {
            let instance_extensions =
                ash_window::enumerate_required_extensions(winit_window).unwrap();
            entry
                .create_instance(
                    &vk::InstanceCreateInfo::builder()
                        .application_info(
                            &vk::ApplicationInfo::builder()
                                .application_name(&CStr::from_bytes_with_nul_unchecked(
                                    b"Dust Application\0",
                                ))
                                .application_version(0)
                                .engine_name(&CStr::from_bytes_with_nul_unchecked(b"Dust Engine\0"))
                                .engine_version(0)
                                .api_version(vk::make_api_version(0, 1, 2, 0)),
                        )
                        .enabled_extension_names(
                            &instance_extensions
                                .iter()
                                .map(|&str| str.as_ptr())
                                .collect::<Vec<_>>(),
                        )
                        .enabled_layer_names(&[]),
                    None,
                )
                .unwrap()
        };
        let surface = ash_window::create_surface(&entry, &instance, winit_window, None).unwrap();
        let (physical_device, device_info) = {
            let available_physical_devices: Vec<_> = instance
                .enumerate_physical_devices()
                .unwrap()
                .into_iter()
                .map(|physical_device| {
                    let device_info = DeviceInfo::new(&entry, &instance, physical_device);
                    (physical_device, device_info)
                })
                .filter(|(_physical_device, device_info)| {
                    device_info.features.sparse_residency_buffer != 0
                        && device_info.features.sparse_binding != 0
                })
                .collect();
            let (physical_device, device_info) = available_physical_devices
                .iter()
                .find(|(_physical_device, device_info)| {
                    device_info.physical_device_properties.device_type
                        == vk::PhysicalDeviceType::DISCRETE_GPU
                })
                .or_else(|| {
                    available_physical_devices
                        .iter()
                        .find(|(_physical_device, device_info)| {
                            device_info.physical_device_properties.device_type
                                == vk::PhysicalDeviceType::INTEGRATED_GPU
                        })
                })
                .expect("Unable to find a supported graphics card");
            let physical_device = *physical_device;
            let device_info = device_info.clone();
            (physical_device, device_info)
        };
        let surface_loader = ash::extensions::khr::Surface::new(&entry, &instance);
        let (graphics_queue_family, compute_queue_family, transfer_binding_queue_family) = {
            let available_queue_family =
                instance.get_physical_device_queue_family_properties(physical_device);
            let graphics_queue_family = available_queue_family
                .iter()
                .enumerate()
                .find(|&(i, family)| {
                    family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        && surface_loader
                            .get_physical_device_surface_support(physical_device, i as u32, surface)
                            .unwrap_or(false)
                })
                .unwrap()
                .0 as u32;
            let compute_queue_family = available_queue_family
                .iter()
                .enumerate()
                .find(|&(_, family)| {
                    !family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        && family.queue_flags.contains(vk::QueueFlags::COMPUTE)
                })
                .or_else(|| {
                    available_queue_family
                        .iter()
                        .enumerate()
                        .find(|&(_, family)| family.queue_flags.contains(vk::QueueFlags::COMPUTE))
                })
                .unwrap()
                .0 as u32;
            let transfer_binding_queue_family = available_queue_family
                .iter()
                .enumerate()
                .find(|&(_, family)| {
                    !family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        && !family.queue_flags.contains(vk::QueueFlags::COMPUTE)
                        && family
                            .queue_flags
                            .contains(vk::QueueFlags::TRANSFER | vk::QueueFlags::SPARSE_BINDING)
                })
                .or_else(|| {
                    available_queue_family
                        .iter()
                        .enumerate()
                        .find(|&(_, family)| {
                            !family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                                && family.queue_flags.contains(
                                    vk::QueueFlags::TRANSFER | vk::QueueFlags::SPARSE_BINDING,
                                )
                        })
                })
                .or_else(|| {
                    available_queue_family
                        .iter()
                        .enumerate()
                        .find(|&(_, family)| {
                            family
                                .queue_flags
                                .contains(vk::QueueFlags::TRANSFER | vk::QueueFlags::SPARSE_BINDING)
                        })
                })
                .unwrap()
                .0 as u32;
            (
                graphics_queue_family,
                compute_queue_family,
                transfer_binding_queue_family,
            )
        };
        let device = instance
            .create_device(
                physical_device,
                &vk::DeviceCreateInfo::builder()
                    .queue_create_infos(&[
                        vk::DeviceQueueCreateInfo::builder()
                            .queue_family_index(graphics_queue_family)
                            .queue_priorities(&[1.0])
                            .build(),
                        vk::DeviceQueueCreateInfo::builder()
                            .queue_family_index(compute_queue_family)
                            .queue_priorities(&[0.1])
                            .build(),
                        vk::DeviceQueueCreateInfo::builder()
                            .queue_family_index(transfer_binding_queue_family)
                            .queue_priorities(&[0.5])
                            .build(),
                    ])
                    .enabled_extension_names(&[
                        ash::extensions::khr::Swapchain::name().as_ptr(),
                        ash::extensions::khr::AccelerationStructure::name().as_ptr(),
                        ash::extensions::khr::DeferredHostOperations::name().as_ptr(),
                        ash::extensions::khr::RayTracingPipeline::name().as_ptr(),
                    ])
                    .enabled_features(&vk::PhysicalDeviceFeatures {
                        sparse_binding: 1,
                        sparse_residency_buffer: 1,
                        ..Default::default()
                    })
                    .push_next(
                        &mut vk::PhysicalDeviceShaderFloat16Int8Features::builder()
                            .shader_int8(false)
                            .build(),
                    )
                    .push_next(
                        &mut vk::PhysicalDevice16BitStorageFeatures::builder()
                            .storage_buffer16_bit_access(true)
                            .build(),
                    )
                    .push_next(
                        &mut vk::PhysicalDevice8BitStorageFeatures::builder()
                            .uniform_and_storage_buffer8_bit_access(true)
                            .build(),
                    )
                    .push_next(
                        &mut vk::PhysicalDeviceBufferDeviceAddressFeatures::builder()
                            .buffer_device_address(true)
                            .build(),
                    )
                    .push_next(
                        &mut vk::PhysicalDeviceAccelerationStructureFeaturesKHR::builder()
                            .acceleration_structure(true)
                            .build(),
                    )
                    .push_next(
                        &mut vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::builder()
                            .ray_tracing_pipeline(true)
                            .build(),
                    ),
                None,
            )
            .unwrap();
        let queues = Queues::new(
            &device,
            graphics_queue_family,
            compute_queue_family,
            transfer_binding_queue_family,
        );

        {
            use gpu_alloc::{Config, DeviceProperties, MemoryHeap, MemoryType};
            use gpu_alloc_ash::memory_properties_from_ash;
            let config = Config::i_am_prototyping();

            let allocator: Allocator = Allocator::new(
                config,
                DeviceProperties {
                    memory_types: Cow::Owned(
                        device_info
                            .memory_types()
                            .iter()
                            .map(|memory_type| MemoryType {
                                props: memory_properties_from_ash(memory_type.property_flags),
                                heap: memory_type.heap_index,
                            })
                            .collect(),
                    ),
                    memory_heaps: Cow::Owned(
                        device_info
                            .memory_heaps()
                            .iter()
                            .map(|&memory_heap| MemoryHeap {
                                size: memory_heap.size,
                            })
                            .collect(),
                    ),
                    max_memory_allocation_count: device_info
                        .physical_device_properties
                        .limits
                        .max_memory_allocation_count,
                    max_memory_allocation_size: u64::MAX,
                    non_coherent_atom_size: device_info
                        .physical_device_properties
                        .limits
                        .non_coherent_atom_size,
                    buffer_device_address: device_info
                        .buffer_device_address_features
                        .buffer_device_address
                        != 0,
                },
            );
            commands.insert_resource(allocator);
        }

        commands.insert_resource(ash::extensions::khr::AccelerationStructure::new(
            &instance, &device,
        ));
        commands.insert_resource(ash::extensions::khr::RayTracingPipeline::new(
            &instance, &device,
        ));
        commands.insert_resource(ash::extensions::khr::DeferredHostOperations::new(
            &instance, &device,
        ));
        commands.insert_resource(entry);
        commands.insert_resource(instance);
        commands.insert_resource(surface);
        commands.insert_resource(surface_loader);
        commands.insert_resource(queues);
        commands.insert_resource(device);
        commands.insert_resource(physical_device);
        commands.insert_resource(device_info);
    }
}
