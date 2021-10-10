use std::mem::MaybeUninit;

use ash::vk;
use bevy::window::RawWindowHandleWrapper;
use std::sync::Arc;

const SWAPCHAIN_LEN: u32 = 3;
const NUM_FRAMES_IN_FLIGHT: u32 = 3;

#[derive(Clone)]
pub struct Frame {
    swapchain_image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    fence: vk::Fence,
}

#[derive(Clone)]
pub struct SwapchainImage {
    image: vk::Image,
    fence: vk::Fence, // This fence was borrowed from the last rendered frame.
    // The reason we need a separate command buffer for each swapchain image
    // is that cmd_begin_render_pass contains a reference to the framebuffer
    // which is unique to each swapchain image.
    pub(crate) bevy_texture: bevy::render2::render_resource::TextureView,
}

pub struct PresentationFrame {
    pub frame: Frame,
    pub swapchain_image: SwapchainImage,
    pub image_index: u32,
}

pub struct SurfaceState {
    surface: vk::SurfaceKHR,
    swapchain: vk::SwapchainKHR,
    format: vk::Format,
    extent: vk::Extent2D,
    current_frame: u32,
    swapchain_images: Vec<SwapchainImage>,
    frames_in_flight: [Frame; NUM_FRAMES_IN_FLIGHT as usize],
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
        let mut frames_in_flight: [MaybeUninit<Frame>; NUM_FRAMES_IN_FLIGHT as usize] =
            MaybeUninit::uninit_array();
        for frame in frames_in_flight.iter_mut() {
            frame.write(Frame {
                swapchain_image_available_semaphore: device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                    .unwrap(),
                render_finished_semaphore: device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                    .unwrap(),
                fence: device
                    .create_fence(
                        &vk::FenceCreateInfo::builder()
                            .flags(vk::FenceCreateFlags::SIGNALED)
                            .build(),
                        None,
                    )
                    .unwrap(),
            });
        }
        Self {
            surface,
            swapchain: vk::SwapchainKHR::null(),
            format: vk::Format::default(),
            extent: vk::Extent2D::default(),
            current_frame: 0,
            swapchain_images: Vec::new(),
            frames_in_flight: std::mem::transmute(frames_in_flight),
        }
    }
    pub unsafe fn destroy_swapchain(
        &mut self,
        device: &ash::Device,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) {
        swapchain_loader.destroy_swapchain(self.swapchain, None);
        self.swapchain = vk::SwapchainKHR::null();
    }
    pub unsafe fn build_swapchain(
        &mut self,
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
        let format = supported_formats[0].format;
        let extent = caps.current_extent;
        self.format = format;
        self.extent = extent;

        let swapchain = swapchain_loader
            .create_swapchain(
                &vk::SwapchainCreateInfoKHR::builder()
                    .surface(self.surface)
                    .min_image_count(SWAPCHAIN_LEN)
                    .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .image_format(format)
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
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
                            None,
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

    pub unsafe fn next_frame(
        &mut self,
        device: &ash::Device,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) -> PresentationFrame {
        assert_ne!(
            self.swapchain,
            vk::SwapchainKHR::null(),
            "SurfaceState: next_frame called without initialized swapchain"
        );
        let frame_in_flight = &self.frames_in_flight[self.current_frame as usize];
        device
            .wait_for_fences(&[frame_in_flight.fence], true, u64::MAX)
            .unwrap();
        let (image_index, suboptimal) = swapchain_loader
            .acquire_next_image(
                self.swapchain,
                u64::MAX,
                frame_in_flight.swapchain_image_available_semaphore,
                vk::Fence::null(),
            )
            .unwrap();
        if suboptimal {
            println!("Suboptimal image acquired.");
        }
        let swapchain_image = &mut self.swapchain_images[image_index as usize];
        {
            if swapchain_image.fence != vk::Fence::null() {
                device
                    .wait_for_fences(&[swapchain_image.fence], true, u64::MAX)
                    .unwrap();
            }
            swapchain_image.fence = frame_in_flight.fence;
        }
        device.reset_fences(&[frame_in_flight.fence]).unwrap();

        self.current_frame = self.current_frame + 1;
        if self.current_frame >= NUM_FRAMES_IN_FLIGHT {
            self.current_frame = 0;
        }
        PresentationFrame {
            swapchain_image: swapchain_image.clone(),
            frame: frame_in_flight.clone(),
            image_index,
        }
    }
}

pub fn render_system(world: &mut bevy::ecs::world::World) {
    use crate::queues::Queues;
    use crate::window::WindowSurfaces;
    use bevy::ecs::prelude::*;
    use bevy::render2::render_graph::RenderGraph;
    use bevy::render2::renderer::{RenderDevice, RenderQueue};
    world.resource_scope(|world, mut graph: Mut<RenderGraph>| {
        graph.update(world);
    });
    let graph = world.get_resource::<RenderGraph>().unwrap();
    let render_device = world.get_resource::<RenderDevice>().unwrap();
    let render_queue = world.get_resource::<RenderQueue>().unwrap();

    /*
    RenderGraphRunner::run(
        graph,
        render_device.clone(), // TODO: is this clone really necessary?
        render_queue,
        world,
    )
    .unwrap();

    */
    {
        let (swapchain_loader, queues, mut windows) = bevy::ecs::system::SystemState::<(
            Res<ash::extensions::khr::Swapchain>,
            Res<Queues>,
            ResMut<WindowSurfaces>,
        )>::new(world)
        .get_mut(world);
        for window in windows.surfaces.values_mut() {
            let presentation_frame = window.next_frame.take().unwrap();

            unsafe {
                swapchain_loader
                    .queue_present(
                        queues.graphics_queue,
                        &vk::PresentInfoKHR::builder()
                            .wait_semaphores(&[presentation_frame.frame.render_finished_semaphore])
                            .swapchains(&[window.state.swapchain])
                            .image_indices(&[presentation_frame.image_index])
                            .build(),
                    )
                    .unwrap();
            }
        }
    }
}
