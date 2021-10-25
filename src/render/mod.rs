mod swapchain;
mod window;

use std::ffi::CStr;

use crate::{device_info::DeviceInfo, Queues};
use ash::vk;
use bevy::app::{App, AppLabel, Plugin};
use bevy::ecs::schedule::{Stage, StageLabel};
use bevy::ecs::world::World;
use bevy::prelude::IntoExclusiveSystem;
pub use window::RenderState;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, AppLabel)]
pub struct RenderApp;

#[derive(Debug, Hash, PartialEq, Eq, Clone, StageLabel)]
pub enum RenderStage {
    Extract,
    Prepare,
    Queue,
    Render,
    Cleanup,
}

pub struct RenderPlugin {
    pub uniform_size: vk::DeviceSize,
}

impl RenderPlugin {
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

    unsafe fn create_instance(&self, entry: &ash::Entry) -> ash::Instance {
        let instance_extensions: Vec<&'static CStr> = self.instance_extensions(entry);
        let api_version = vk::make_api_version(0, 1, 2, 0);
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
            .unwrap()
    }

    unsafe fn create_physical_device(
        &self,
        entry: &ash::Entry,
        instance: &ash::Instance,
    ) -> (vk::PhysicalDevice, crate::DeviceInfo) {
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
    }

    unsafe fn find_queue_families(
        &self,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
    ) -> (u32, u32, u32) {
        let available_queue_family =
            instance.get_physical_device_queue_family_properties(physical_device);
        let graphics_queue_family = available_queue_family
            .iter()
            .enumerate()
            .find(|&(_i, family)| {
                // Select the first graphics queue family
                family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            })
            .unwrap()
            .0 as u32;
        let compute_queue_family = available_queue_family
            .iter()
            .enumerate()
            .find(|&(_, family)| {
                // Prioritize dedicated compute family without graphics capability
                !family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                    && family.queue_flags.contains(vk::QueueFlags::COMPUTE)
            })
            .or_else(|| {
                // Use first compute-capable queue family
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
                // Prioritize dedicated TRANSFER & SPARSE_BINDING family
                !family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                    && !family.queue_flags.contains(vk::QueueFlags::COMPUTE)
                    && family
                        .queue_flags
                        .contains(vk::QueueFlags::TRANSFER | vk::QueueFlags::SPARSE_BINDING)
            })
            .or_else(|| {
                // Accepts COMPUTE-capable family as well
                available_queue_family
                    .iter()
                    .enumerate()
                    .find(|&(_, family)| {
                        !family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                            && family
                                .queue_flags
                                .contains(vk::QueueFlags::TRANSFER | vk::QueueFlags::SPARSE_BINDING)
                    })
            })
            .or_else(|| {
                // Fallback to general purpose family
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
    }

    unsafe fn create_device(
        &self,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
    ) -> (ash::Device, crate::Queues) {
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

        let (graphics_queue_family, compute_queue_family, transfer_binding_queue_family) =
            self.find_queue_families(&instance, physical_device);
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
        (device, queues)
    }

    unsafe fn create_allocator(&self, device_info: &DeviceInfo) -> crate::Allocator {
        use crate::Allocator;
        use gpu_alloc::{Config, DeviceProperties, MemoryHeap, MemoryType};
        use gpu_alloc_ash::memory_properties_from_ash;
        use std::borrow::Cow;
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
}

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        use bevy::ecs::schedule::SystemStage;
        app.init_resource::<ScratchRenderWorld>();

        let mut render_app = App::empty();
        render_app
            .add_stage(RenderStage::Extract, SystemStage::parallel())
            .add_stage(RenderStage::Prepare, SystemStage::parallel())
            .add_stage(RenderStage::Queue, SystemStage::parallel())
            .add_stage(
                RenderStage::Render,
                SystemStage::parallel().with_system(window::render_system.exclusive_system()),
            )
            .add_stage(RenderStage::Cleanup, SystemStage::parallel());

        unsafe {
            let entry = ash::Entry::new().unwrap();

            let instance = self.create_instance(&entry);
            let (physical_device, device_info) = self.create_physical_device(&entry, &instance);
            let (device, queues) = self.create_device(&instance, physical_device);

            render_app
                .insert_resource(ash::extensions::khr::AccelerationStructure::new(
                    &instance, &device,
                ))
                .insert_resource(ash::extensions::khr::RayTracingPipeline::new(
                    &instance, &device,
                ))
                .insert_resource(ash::extensions::khr::DeferredHostOperations::new(
                    &instance, &device,
                ))
                .insert_resource(ash::extensions::khr::Swapchain::new(&instance, &device))
                .insert_resource(ash::extensions::khr::Surface::new(&entry, &instance))
                .insert_resource(self.create_allocator(&device_info))
                .insert_resource(entry)
                .insert_resource(instance)
                .insert_resource(physical_device)
                .insert_resource(device_info)
                .insert_resource(device)
                .insert_resource(queues);
        }

        app.add_sub_app(RenderApp, render_app, |app_world, render_app| {
            let meta_len = app_world.entities().meta.len();
            render_app
                .world
                .entities()
                .reserve_entities(meta_len as u32);

            // flushing as "invalid" ensures that app world entities aren't added as "empty archetype" entities by default
            // these entities cannot be accessed without spawning directly onto them
            // this _only_ works as expected because clear_entities() is called at the end of every frame.
            render_app.world.entities_mut().flush_as_invalid();
            extract(app_world, render_app);
            {
                let prepare = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Prepare)
                    .unwrap();
                prepare.run(&mut render_app.world);
            }
            {
                let queue = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Queue)
                    .unwrap();
                queue.run(&mut render_app.world);
            }
            {
                let render = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Render)
                    .unwrap();
                render.run(&mut render_app.world);
            }
            {
                let cleanup = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Cleanup)
                    .unwrap();
                cleanup.run(&mut render_app.world);

                render_app.world.clear_entities();
            }
        });

        app.add_plugin(window::WindowRenderPlugin {
            uniform_buffer_size: self.uniform_size,
        });
    }
}

/// A "scratch" world used to avoid allocating new worlds every frame when
// swapping out the Render World.
#[derive(Default)]
struct ScratchRenderWorld(bevy::ecs::world::World);

/// The Render App World. This is only available as a resource during the Extract step.
#[derive(Default)]
pub struct RenderWorld(World);

impl std::ops::Deref for RenderWorld {
    type Target = World;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for RenderWorld {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn extract(app_world: &mut bevy::ecs::world::World, render_app: &mut App) {
    use bevy::ecs::schedule::SystemStage;
    let extract = render_app
        .schedule
        .get_stage_mut::<SystemStage>(&RenderStage::Extract)
        .unwrap();

    // temporarily add the render world to the app world as a resource
    let scratch_world = app_world.remove_resource::<ScratchRenderWorld>().unwrap();
    let render_world = std::mem::replace(&mut render_app.world, scratch_world.0);
    app_world.insert_resource(RenderWorld(render_world));

    extract.run(app_world);

    // add the render world back to the render app
    let render_world = app_world.remove_resource::<RenderWorld>().unwrap();
    let scratch_world = std::mem::replace(&mut render_app.world, render_world.0);
    app_world.insert_resource(ScratchRenderWorld(scratch_world));

    extract.apply_buffers(&mut render_app.world);
}
