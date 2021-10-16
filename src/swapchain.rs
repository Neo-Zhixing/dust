use std::mem::MaybeUninit;

use ash::vk;
use bevy::window::RawWindowHandleWrapper;
use std::sync::Arc;

use crate::window::Frame;

const SWAPCHAIN_LEN: u32 = 3;
use crate::window::NUM_FRAMES_IN_FLIGHT;

#[derive(Clone)]
pub struct SwapchainImage {
    image: vk::Image,
    fence: vk::Fence, // This fence was borrowed from the last rendered frame.
    // The reason we need a separate command buffer for each swapchain image
    // is that cmd_begin_render_pass contains a reference to the framebuffer
    // which is unique to each swapchain image.
    pub(crate) bevy_texture: bevy::render2::render_resource::TextureView,
}

pub struct SurfaceState {  // This is per-window
    surface: vk::SurfaceKHR,
    swapchain: vk::SwapchainKHR,
    format: vk::Format,
    extent: vk::Extent2D,
    swapchain_images: Vec<SwapchainImage>,
    image_available_semaphore: [vk::Semaphore; NUM_FRAMES_IN_FLIGHT as usize], // This should really be per frame, per window
}

impl SurfaceState {
    pub unsafe fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        device: &ash::Device,
        window_handle: &RawWindowHandleWrapper,
    ) -> Self {
        let window_handle = window_handle.get_handle();
        let surface = ash_window::create_surface(entry, instance, &window_handle, None).unwrap();

        let mut image_available_semaphore = [vk::Semaphore::null(); NUM_FRAMES_IN_FLIGHT as usize];
        for semaphore in image_available_semaphore.iter_mut() {
            *semaphore = unsafe{ device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap() };
        }
        Self {
            surface,
            swapchain: vk::SwapchainKHR::null(),
            format: vk::Format::default(),
            extent: vk::Extent2D::default(),
            swapchain_images: Vec::new(),
            image_available_semaphore,
        }
    }
    pub unsafe fn destroy_swapchain(
        &mut self,
        _device: &ash::Device,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) {
        swapchain_loader.destroy_swapchain(self.swapchain, None);
        self.swapchain = vk::SwapchainKHR::null();
    }
    pub unsafe fn build_swapchain(
        &mut self,
        instance: &ash::Instance,
        surface_loader: &ash::extensions::khr::Surface,
        swapchain_loader: &ash::extensions::khr::Swapchain,
        physical_device: vk::PhysicalDevice,
        wgpu_device: &wgpu::Device,
    ) {
        let caps = surface_loader
            .get_physical_device_surface_capabilities(physical_device, self.surface)
            .unwrap();
        let supported_formats = surface_loader
            .get_physical_device_surface_formats(physical_device, self.surface)
            .unwrap();
        let format = supported_formats
            .iter()
            .find(|&format| {
                let properties =
                    instance.get_physical_device_format_properties(physical_device, format.format);
                properties.optimal_tiling_features.contains(
                    vk::FormatFeatureFlags::COLOR_ATTACHMENT
                        | vk::FormatFeatureFlags::STORAGE_IMAGE,
                )
            })
            .expect("Unable to find format that supports color attachment AND storage image");
        println!("Selected format {:?}", format.format);
        let extent = caps.current_extent;
        self.format = format.format;
        self.extent = extent;

        let swapchain = swapchain_loader
            .create_swapchain(
                &vk::SwapchainCreateInfoKHR::builder()
                    .surface(self.surface)
                    .min_image_count(SWAPCHAIN_LEN)
                    .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .image_format(format.format)
                    .image_extent(extent)
                    .image_usage(
                        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::STORAGE,
                    )
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                    .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                    .present_mode(vk::PresentModeKHR::IMMEDIATE)
                    .clipped(true)
                    .image_array_layers(1)
                    .build(),
                None,
            )
            .unwrap();
        self.swapchain = swapchain;

        let images = swapchain_loader.get_swapchain_images(swapchain).unwrap();
        let wgpu_format = wgpu::TextureFormat::Bgra8Unorm; //TODO: implement properly

        let hal_texture_desc = wgpu_hal::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: extent.width,
                height: extent.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu_format,
            usage: wgpu_hal::TextureUses::COLOR_TARGET | wgpu_hal::TextureUses::STORAGE_WRITE,
            memory_flags: wgpu_hal::MemoryFlags::empty(),
        };

        let wgpu_texture_desc = wgpu::TextureDescriptor {
            label: None,
            size: hal_texture_desc.size,
            mip_level_count: hal_texture_desc.mip_level_count,
            sample_count: hal_texture_desc.sample_count,
            dimension: hal_texture_desc.dimension,
            format: hal_texture_desc.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::STORAGE_BINDING,
        };

        self.swapchain_images = images
            .iter()
            .map(|&image| {
                let wgpu_texture = unsafe {
                    let hal_texture =
                        <wgpu_hal::api::Vulkan as wgpu_hal::Api>::Device::texture_from_raw(
                            image,
                            &hal_texture_desc,
                            Some(Box::new(0)), // So that WGPU doesn't drop our textures
                        );

                    wgpu_device.create_texture_from_hal::<wgpu_hal::api::Vulkan>(
                        hal_texture,
                        &wgpu_texture_desc,
                    )
                };
                let wgpu_texture_view = wgpu_texture.create_view(&wgpu::TextureViewDescriptor {
                    label: None,
                    format: None,
                    dimension: None,
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                });
                let bevy_texture = bevy::render2::render_resource::TextureView::from_wgpu(
                    Arc::new(wgpu_texture),
                    Arc::new(wgpu_texture_view),
                );
                SwapchainImage {
                    image: image,
                    fence: vk::Fence::null(),
                    bevy_texture,
                }
            })
            .collect();
    }

    pub unsafe fn next_image(
        &mut self,
        device: &ash::Device,
        frame_in_flight: &Frame,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) -> (SwapchainImage, u32) {
        assert_ne!(
            self.swapchain,
            vk::SwapchainKHR::null(),
            "SurfaceState: next_frame called without initialized swapchain"
        );
        let (image_index, suboptimal) = swapchain_loader
            .acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.image_available_semaphore[frame_in_flight.index as usize],
                vk::Fence::null(),
            )
            .unwrap();
        if suboptimal {
            println!("Suboptimal image acquired.");
        }
        let swapchain_image = &mut self.swapchain_images[image_index as usize];
        {
            if swapchain_image.fence != vk::Fence::null() && swapchain_image.fence != frame_in_flight.fence {
                // Make sure that the previous frame using the current swapchain image finishes rendering
                device
                    .wait_for_fences(&[swapchain_image.fence], true, u64::MAX)
                    .unwrap();
            }
            swapchain_image.fence = frame_in_flight.fence;
        }
        (swapchain_image.clone(), image_index)
    }
}

pub fn render_system(world: &mut bevy::ecs::world::World) {
    use crate::queues::Queues;
    use crate::window::RenderState;
    use bevy::ecs::prelude::*;
    use bevy::render2::render_graph::RenderGraph;
    use bevy::render2::renderer::{RenderDevice, RenderGraphRunner, RenderQueue};
    world.resource_scope(|world, mut graph: Mut<RenderGraph>| {
        graph.update(world);
    });
    let graph = world.get_resource::<RenderGraph>().unwrap();
    let render_device = world.get_resource::<RenderDevice>().unwrap();
    let render_queue = world.get_resource::<RenderQueue>().unwrap();
    let render_state = world.get_resource::<RenderState>().unwrap();
    let command_buffer = render_state.current_frame().command_buffer;

    let frame_in_flight = render_state.current_frame().clone();
    let swapchain_image_available_semaphores: Vec<vk::Semaphore> = render_state.surfaces.values().map(|surface| surface.state.image_available_semaphore[frame_in_flight.index as usize]).collect();


    assert_eq!(swapchain_image_available_semaphores.len(), 1, "For now we only supports one surface");
    let command_encoder =
    render_device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut render_context = bevy::render2::renderer::RenderContext {
        render_device: render_device.clone(),
        command_encoder,
    };
    RenderGraphRunner::run_graph(graph, None, &mut render_context, world, &[]).unwrap();

    render_queue.as_hal::<wgpu_hal::api::Vulkan, _, ()>(|queue| {
        queue.relay_active = false;
        queue.render_complete_semaphore = frame_in_flight.render_finished_semaphore;
        queue.image_available_semaphore = swapchain_image_available_semaphores[0];//TODO
        queue.fence_override = frame_in_flight.fence;
    });
    render_queue.submit(vec![render_context.command_encoder.finish()]);
    render_queue.as_hal::<wgpu_hal::api::Vulkan, _, ()>(|queue| {
        queue.relay_active = false;
        queue.render_complete_semaphore = vk::Semaphore::null();
        queue.image_available_semaphore = vk::Semaphore::null();
        queue.fence_override = vk::Fence::null();
    });

    {
        let (device, swapchain_loader, queues, mut render_state) = bevy::ecs::system::SystemState::<(
            Res<ash::Device>,
            Res<ash::extensions::khr::Swapchain>,
            Res<Queues>,
            ResMut<RenderState>,
        )>::new(world)
        .get_mut(world);
        for window in render_state.surfaces.values_mut() {
            let (swapchain_image, image_index) = window.next_frame.take().unwrap();

            unsafe {
                swapchain_loader
                    .queue_present(
                        queues.graphics_queue,
                        &vk::PresentInfoKHR::builder()
                            .wait_semaphores(&[frame_in_flight.render_finished_semaphore])
                            .swapchains(&[window.state.swapchain])
                            .image_indices(&[image_index])
                            .build(),
                    )
                    .unwrap();
            }
        }
    }
}
