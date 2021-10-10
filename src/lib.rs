#![feature(maybe_uninit_uninit_array)]
#![feature(alloc_layout_extra)]
#![feature(untagged_unions)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(const_cstr_unchecked)]
#![feature(adt_const_params)]

mod device_info;
mod fps_counter;
mod queues;
mod raytrace;
mod util;
mod window;
mod swapchain;

use bevy::render2::RenderApp;
use device_info::DeviceInfo;

use bevy::prelude::*;

use ash::vk;
use std::borrow::Cow;
use std::ffi::CStr;

pub type Allocator = gpu_alloc::GpuAllocator<vk::DeviceMemory>;
pub type MemoryBlock = gpu_alloc::MemoryBlock<vk::DeviceMemory>;
pub use queues::Queues;
pub use raytrace::Raytraced;
pub use raytrace::VoxelModel;

#[derive(Default)]
pub struct DustPlugin;

impl Plugin for DustPlugin {
    fn build(&self, app: &mut App) {
        let state = self.initialize(app);
        app.add_plugin(bevy::render2::RenderPlugin {
            enable_render_system: false,
            enable_window_plugin: false,
        })
            .add_plugin(bevy::core_pipeline::CorePipelinePlugin::default())
            .add_plugin(bevy::pbr2::PbrPlugin::default());

        self.extract(app, state);
        app
        .add_plugin(window::WindowRenderPlugin::default())
        .add_plugin(raytrace::RaytracePlugin::default())
            .insert_resource(fps_counter::FPSCounter::default())
            .add_system(fps_counter::fps_counter);
        
        app.sub_app(RenderApp)
        .add_system_to_stage(bevy::render2::RenderStage::Render, swapchain::render_system.exclusive_system());
    }
}
impl DustPlugin {
    fn initialize(
        &self,
        app: &mut App,
    ) -> (
        ash::Entry,
        ash::Device,
        Queues,
        ash::Instance,
        vk::PhysicalDevice,
        DeviceInfo,
    ) {
        unsafe {
            let entry = ash::Entry::new().unwrap();
            let api_version = vk::make_api_version(0, 1, 2, 0);

            let instance_extensions = self.instance_extensions(&entry);
            let instance = entry
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
                                .api_version(api_version),
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
                .unwrap();

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
            let (graphics_queue_family, compute_queue_family, transfer_binding_queue_family) = {
                let available_queue_family =
                    instance.get_physical_device_queue_family_properties(physical_device);
                let graphics_queue_family = available_queue_family
                    .iter()
                    .enumerate()
                    .find(|&(i, family)| {
                        family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        // TODO && surface_loader
                        //     .get_physical_device_surface_support(physical_device, i as u32, surface)
                        //    .unwrap_or(false)
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
                            .find(|&(_, family)| {
                                family.queue_flags.contains(vk::QueueFlags::COMPUTE)
                            })
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
                                family.queue_flags.contains(
                                    vk::QueueFlags::TRANSFER | vk::QueueFlags::SPARSE_BINDING,
                                )
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
            let device_extensions = [
                ash::extensions::khr::Swapchain::name(),
                ash::extensions::khr::AccelerationStructure::name(),
                ash::extensions::khr::DeferredHostOperations::name(),
                ash::extensions::khr::RayTracingPipeline::name(),
            ];
            let mut device_extensions_ptrs: [*const i8; 4] = [std::ptr::null(); 4];
            for (ptr, &cstr) in device_extensions_ptrs
                .iter_mut()
                .zip(device_extensions.iter())
            {
                *ptr = cstr.as_ptr();
            }
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
                        .enabled_extension_names(&device_extensions_ptrs)
                        .enabled_features(&vk::PhysicalDeviceFeatures {
                            sparse_binding: 1,
                            sparse_residency_buffer: 1,
                            shader_storage_image_write_without_format: 1,
                            image_cube_array: 1,
                            ..Default::default()
                        })
                        .push_next(
                            &mut vk::PhysicalDeviceVulkan12Features::builder()
                                .shader_int8(true)
                                .uniform_and_storage_buffer8_bit_access(true)
                                .buffer_device_address(true)
                                .timeline_semaphore(true)
                                .imageless_framebuffer(true)
                                .descriptor_indexing(true)
                                .build(),
                        )
                        .push_next(
                            &mut vk::PhysicalDevice16BitStorageFeatures::builder()
                                .storage_buffer16_bit_access(true)
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
                        )
                        .build(),
                    None,
                )
                .unwrap();
            let queues = Queues::new(
                &device,
                graphics_queue_family,
                compute_queue_family,
                transfer_binding_queue_family,
            );

            app.insert_resource(self.create_wgpu_renderer(
                &entry,
                &device,
                &instance,
                api_version,
                instance_extensions,
                &device_extensions,
                physical_device,
                &queues,
            ));

            (
                entry,
                device,
                queues,
                instance,
                physical_device,
                device_info,
            )
        }
    }

    fn extract(
        &self,
        app: &mut App,
        state: (
            ash::Entry,
            ash::Device,
            Queues,
            ash::Instance,
            vk::PhysicalDevice,
            DeviceInfo,
        ),
    ) {
        let sub_app = app.sub_app(RenderApp);
        let (entry, device, queues, instance, physical_device, device_info) = state;
        let desc_pool = unsafe {
            // This desc pool will be added as a resource to render world
            // Change this whenever you need space for a new global desc set
            device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::builder()
                        .flags(vk::DescriptorPoolCreateFlags::empty())
                        .max_sets(2)
                        .pool_sizes(&[
                            vk::DescriptorPoolSize {
                                ty: vk::DescriptorType::STORAGE_TEXEL_BUFFER,
                                descriptor_count: 1,
                            },
                            vk::DescriptorPoolSize {
                                ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                                descriptor_count: 1,
                            },
                        ])
                        .build(),
                    None,
                )
                .unwrap()
        };

        sub_app
            .insert_resource(ash::extensions::khr::AccelerationStructure::new(
                &instance, &device,
            ))
            .insert_resource(ash::extensions::khr::RayTracingPipeline::new(
                &instance, &device,
            ))
            .insert_resource(ash::extensions::khr::DeferredHostOperations::new(
                &instance, &device,
            ))
            .insert_resource(desc_pool)
            .insert_resource(queues)
            .insert_resource(device)
            .insert_resource(entry)
            .insert_resource(instance)
            .insert_resource(physical_device);

        unsafe {
            sub_app
                .insert_resource(self.create_allocator(&device_info))
                .insert_resource(device_info);
        }
    }
    #[allow(unused_variables)]
    fn instance_extensions(&self, entry: &ash::Entry) -> Vec<&'static CStr> {
        use ash::extensions::*;
        let mut extensions: Vec<&'static CStr> = Vec::with_capacity(5);
        extensions.push(khr::Surface::name());
        extensions.push(vk::KhrGetPhysicalDeviceProperties2Fn::name());
        #[cfg(target_os = "windows")]
        extensions.push(khr::Win32Surface::name());

        #[cfg(all(unix, not(target_os = "android"), not(target_os = "macos")))]
        {
            let unix_extensions: [&'static CStr; 3] = [
                khr::XlibSurface::name(),
                khr::XcbSurface::name(),
                khr::WaylandSurface::name(),
            ];
            let available_extensions = entry.enumerate_instance_extension_properties().unwrap();
            for extension in unix_extensions {
                if available_extensions.iter().any(|inst_ext| unsafe {
                    CStr::from_ptr(inst_ext.extension_name.as_ptr()) == extension
                }) {
                    extensions.push(extension);
                }
            }
        }

        #[cfg(target_os = "macos")]
        extensions.push(ext::MetalSurface::name());

        extensions
    }
    unsafe fn create_allocator(&self, device_info: &DeviceInfo) -> Allocator {
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
                max_memory_allocation_size: u64::MAX, // TODO
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
        allocator
    }
    unsafe fn create_wgpu_renderer(
        &self,
        entry: &ash::Entry,
        device: &ash::Device,
        instance: &ash::Instance,
        driver_api_version: u32,
        instance_extensions: Vec<&'static CStr>,
        device_extensions: &[&'static CStr],
        physical_device: vk::PhysicalDevice,
        queue: &Queues,
    ) -> bevy::render2::renderer::Renderer {
        use std::sync::Arc;
        use wgpu_hal as hal;

        let nv_optimus_layer = CStr::from_bytes_with_nul(b"VK_LAYER_NV_optimus\0").unwrap();
        let has_nv_optimus = entry.enumerate_instance_layer_properties().unwrap()
            .iter()
            .any(|inst_layer| CStr::from_ptr(inst_layer.layer_name.as_ptr()) == nv_optimus_layer);


        let hal_instance = <hal::api::Vulkan as hal::Api>::Instance::from_raw(
            entry.clone(),
            instance.clone(),
            driver_api_version,
            instance_extensions,
            hal::InstanceFlags::empty(), // TODO: enable debug flags
            has_nv_optimus,
            None,
        )
        .unwrap();
        let hal_exposed_adapter = hal_instance
            .expose_adapter(physical_device)
            .expect("Unable to obtain adapter");

        let wgpu_instance = wgpu::Instance::from_hal::<hal::api::Vulkan>(hal_instance);

        let phd_properties = instance.get_physical_device_properties(physical_device);
        let phd_features = &hal_exposed_adapter.adapter.phd_features;
        let phd_capabilities = &hal_exposed_adapter.adapter.phd_capabilities;
        let limits = phd_capabilities.to_wgpu_limits(phd_features);
        let hal_device = hal_exposed_adapter
            .adapter
            .device_from_raw(
                device.clone(),
                false,
                device_extensions,
                wgpu_hal::vulkan::UpdateAfterBindTypes::from_limits(
                    &limits,
                    &phd_properties.limits,
                ),
                queue.graphics_queue_family,
                0,
            )
            .unwrap();

        let adapter = wgpu_instance.create_adapter_from_hal(hal_exposed_adapter);
        let (device, queue) = adapter
            .create_device_from_hal(
                hal_device,
                &wgpu::DeviceDescriptor {
                    label: Some("ajaja"),
                    features: wgpu::Features::default(),
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .unwrap();
        bevy::render2::renderer::Renderer {
            instance: wgpu_instance,
            device: Arc::new(device).into(),
            queue: Arc::new(queue),
        }
    }
}
