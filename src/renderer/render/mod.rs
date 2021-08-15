mod commands;
use std::mem::MaybeUninit;

use ash::vk;
use bevy::prelude::*;

use super::Queues;

const NUM_FRAMES_IN_FLIGHT: usize = 3;
const SWAPCHAIN_LEN: u32 = 3;
struct Frame {
    swapchain_image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    fence: vk::Fence,
}
pub struct SwapchainImage {
    pub view: vk::ImageView,
    pub image: vk::Image,
    fence: vk::Fence, // This fence was borrowed from the last rendered frame.
    // The reason we need a separate command buffer for each swapchain image
    // is that cmd_begin_render_pass contains a reference to the framebuffer
    // which is unique to each swapchain image.
    pub command_buffer: vk::CommandBuffer,
}
pub(super) struct RenderState {
    current_frame: usize,
    frames_in_flight: [Frame; NUM_FRAMES_IN_FLIGHT],
    swapchain_images: [SwapchainImage; SWAPCHAIN_LEN as usize],
    format: vk::Format,
    extent: vk::Extent2D,
    swapchain: vk::SwapchainKHR,
    command_pool: vk::CommandPool,
}

impl RenderState {
    unsafe fn new(device: &ash::Device, graphics_queue_family_index: u32) -> RenderState {
        let command_pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::builder()
                    .queue_family_index(graphics_queue_family_index)
                    .flags(vk::CommandPoolCreateFlags::empty())
                    .build(),
                None,
            )
            .unwrap();
        let command_buffers = {
            let mut command_buffers = [vk::CommandBuffer::null(); SWAPCHAIN_LEN as usize];
            let result = device.fp_v1_0().allocate_command_buffers(
                device.handle(),
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .command_buffer_count(SWAPCHAIN_LEN)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .build(),
                command_buffers.as_mut_ptr(),
            );
            assert_eq!(result, vk::Result::SUCCESS);
            command_buffers
        };
        let mut frames_in_flight: [MaybeUninit<Frame>; NUM_FRAMES_IN_FLIGHT] =
            MaybeUninit::uninit().assume_init();
        for i in 0..NUM_FRAMES_IN_FLIGHT {
            let frame = Frame {
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
            };
            frames_in_flight[i].write(frame);
        }

        let mut swapchain_images: [MaybeUninit<SwapchainImage>; SWAPCHAIN_LEN as usize] =
            MaybeUninit::uninit_array();
        for (image, &command_buffer) in swapchain_images.iter_mut().zip(command_buffers.iter()) {
            image.write(SwapchainImage {
                view: vk::ImageView::null(),
                image: vk::Image::null(),
                fence: vk::Fence::null(),
                command_buffer,
            });
        }

        RenderState {
            current_frame: 0,
            frames_in_flight: std::mem::transmute(frames_in_flight),
            swapchain_images: std::mem::transmute(swapchain_images),
            format: vk::Format::default(),
            extent: vk::Extent2D::default(),
            swapchain: vk::SwapchainKHR::null(),
            command_pool,
        }
    }
    unsafe fn destroy_swapchain(
        &mut self,
        device: &ash::Device,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) {
        for image in self.swapchain_images.iter_mut() {
            device.destroy_image_view(image.view, None);
            image.view = vk::ImageView::null();
            image.image = vk::Image::null();
        }
        swapchain_loader.destroy_swapchain(self.swapchain, None);
        self.swapchain = vk::SwapchainKHR::null();
        self.format = Default::default();
        self.extent = Default::default();
    }
    unsafe fn reset_commands(&self, device: &ash::Device) {
        device
            .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())
            .unwrap();
    }
    unsafe fn build_swapchain(
        &mut self,
        device: &ash::Device,
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::extensions::khr::Surface,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) {
        let caps = surface_loader
            .get_physical_device_surface_capabilities(physical_device, surface)
            .unwrap();
        let supported_formats = surface_loader
            .get_physical_device_surface_formats(physical_device, surface)
            .unwrap();
        let format = supported_formats[0].format;
        let extent = caps.current_extent;
        self.format = format;
        self.extent = extent;

        let swapchain = swapchain_loader
            .create_swapchain(
                &vk::SwapchainCreateInfoKHR::builder()
                    .surface(surface)
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

        let images = {
            let mut img_count: u32 = SWAPCHAIN_LEN;
            let mut images = [vk::Image::null(); SWAPCHAIN_LEN as usize];
            let result = swapchain_loader.fp().get_swapchain_images_khr(
                swapchain_loader.device(),
                swapchain,
                &mut img_count,
                images.as_mut_ptr(),
            );
            assert_eq!(img_count, SWAPCHAIN_LEN);
            assert_eq!(result, vk::Result::SUCCESS);
            images
        };
        for (&image, swapchain_image) in images.iter().zip(self.swapchain_images.iter_mut()) {
            swapchain_image.image = image;
            swapchain_image.view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::builder()
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .components(vk::ComponentMapping {
                            r: vk::ComponentSwizzle::R,
                            g: vk::ComponentSwizzle::G,
                            b: vk::ComponentSwizzle::B,
                            a: vk::ComponentSwizzle::A,
                        })
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                        .image(image),
                    None,
                )
                .unwrap();
        }
    }
}

pub(super) fn setup(
    mut commands: Commands,
    instance: Res<ash::Instance>,
    device: Res<ash::Device>,
    physical_device: Res<vk::PhysicalDevice>,
    surface: Res<vk::SurfaceKHR>,
    surface_loader: Res<ash::extensions::khr::Surface>,
    queues: Res<Queues>,
) {
    let instance = &*instance;
    let device = &*device;
    let surface = *surface;
    let physical_device = *physical_device;

    unsafe {
        let swapchain_loader = ash::extensions::khr::Swapchain::new(instance, device);
        let mut render_state = RenderState::new(device, queues.graphics_queue_family);
        render_state.build_swapchain(
            device,
            physical_device,
            surface,
            &surface_loader,
            &swapchain_loader,
        );
        commands::record_command_buffers(&device, &render_state);
        commands.insert_resource(swapchain_loader);
        commands.insert_resource(render_state);
    }
}

pub(super) fn rebuild(
    mut window_resized_events: EventReader<bevy::window::WindowResized>,
    device: Res<ash::Device>,
    mut render_state: ResMut<RenderState>,
    swapchain_loader: Res<ash::extensions::khr::Swapchain>,
    surface: Res<vk::SurfaceKHR>,
    physical_device: Res<vk::PhysicalDevice>,
    surface_loader: Res<ash::extensions::khr::Surface>,
    queues: Res<Queues>,
) {
    if window_resized_events.iter().next().is_none() {
        return;
    }
    let device = &*device;
    let swapchain_loader = &*swapchain_loader;
    let surface_loader = &*surface_loader;
    let physical_device = *physical_device;
    let surface = *surface;
    unsafe {
        device.queue_wait_idle(queues.graphics_queue).unwrap();
        render_state.destroy_swapchain(device, swapchain_loader);
        render_state.build_swapchain(
            device,
            physical_device,
            surface,
            surface_loader,
            swapchain_loader,
        );
        render_state.reset_commands(device);
        commands::record_command_buffers(&device, &render_state);
    }
}

pub(super) fn update(
    mut render_state: ResMut<RenderState>,
    device: Res<ash::Device>,
    swapchain_loader: Res<ash::extensions::khr::Swapchain>,
    queues: Res<Queues>,
) {
    let render_state = &mut *render_state;
    let swapchain = render_state.swapchain;
    unsafe {
        let frame_in_flight = &render_state.frames_in_flight[render_state.current_frame];
        device
            .wait_for_fences(&[frame_in_flight.fence], true, u64::MAX)
            .unwrap();
        let (image_index, suboptimal) = swapchain_loader
            .acquire_next_image(
                swapchain,
                u64::MAX,
                frame_in_flight.swapchain_image_available_semaphore,
                vk::Fence::null(),
            )
            .unwrap();
        if suboptimal {
            println!("Suboptimal image acquired.");
        }
        let swapchain_image = &mut render_state.swapchain_images[image_index as usize];
        {
            if swapchain_image.fence != vk::Fence::null() {
                device
                    .wait_for_fences(&[swapchain_image.fence], true, u64::MAX)
                    .unwrap();
            }
            swapchain_image.fence = frame_in_flight.fence;
        }
        device.reset_fences(&[frame_in_flight.fence]).unwrap();
        device
            .queue_submit(
                queues.graphics_queue,
                &[vk::SubmitInfo::builder()
                    .wait_semaphores(&[frame_in_flight.swapchain_image_available_semaphore])
                    .signal_semaphores(&[frame_in_flight.render_finished_semaphore])
                    .command_buffers(&[swapchain_image.command_buffer])
                    .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
                    .build()],
                frame_in_flight.fence,
            )
            .unwrap();
        swapchain_loader
            .queue_present(
                queues.graphics_queue,
                &vk::PresentInfoKHR::builder()
                    .wait_semaphores(&[frame_in_flight.render_finished_semaphore])
                    .swapchains(&[swapchain])
                    .image_indices(&[image_index]),
            )
            .unwrap();
        render_state.current_frame = render_state.current_frame + 1;
        if render_state.current_frame >= render_state.frames_in_flight.len() {
            render_state.current_frame = 0;
        }
    }
}
