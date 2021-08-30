use ash::vk;
use core::mem::MaybeUninit;
use std::result;

const NUM_FRAMES_IN_FLIGHT: usize = 3;
const SWAPCHAIN_LEN: u32 = 3;
pub(super) struct Frame {
    pub(super) swapchain_image_available_semaphore: vk::Semaphore,
    pub(super) render_finished_semaphore: vk::Semaphore,
    pub(super) fence: vk::Fence,
}
pub(super) struct SwapchainImage {
    pub(super) view: vk::ImageView,
    pub(super) image: vk::Image,
    pub(super) fence: vk::Fence, // This fence was borrowed from the last rendered frame.
    // The reason we need a separate command buffer for each swapchain image
    // is that cmd_begin_render_pass contains a reference to the framebuffer
    // which is unique to each swapchain image.
    pub(super) command_buffer: vk::CommandBuffer,
    pub(super) image_desc_set: vk::DescriptorSet,
}
pub struct RenderState {
    pub(super) current_frame: usize,
    pub(super) frames_in_flight: [Frame; NUM_FRAMES_IN_FLIGHT],
    pub(super) swapchain_images: [SwapchainImage; SWAPCHAIN_LEN as usize],
    pub(super) swapchain_images_desc_set_layout: vk::DescriptorSetLayout,
    pub(super) format: vk::Format,
    pub(super) extent: vk::Extent2D,
    pub(super) swapchain: vk::SwapchainKHR,
    pub(super) command_pool: vk::CommandPool,
}

impl RenderState {
    pub(super) unsafe fn new(
        device: &ash::Device,
        graphics_queue_family_index: u32,
    ) -> RenderState {
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

        let target_img_desc_layout = device
            .create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::builder()
                    .bindings(&[vk::DescriptorSetLayoutBinding::builder()
                        .binding(0)
                        .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
                        .build()])
                    .build(),
                None,
            )
            .unwrap();
        let target_img_desc_pool = device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::builder()
                    .flags(vk::DescriptorPoolCreateFlags::empty())
                    .max_sets(SWAPCHAIN_LEN as u32)
                    .pool_sizes(&[vk::DescriptorPoolSize::builder()
                        .ty(vk::DescriptorType::STORAGE_IMAGE)
                        .descriptor_count(3)
                        .build()])
                    .build(),
                None,
            )
            .unwrap();

        let mut target_img_descs = [vk::DescriptorSet::default(); SWAPCHAIN_LEN as usize];
        let set_layouts = [target_img_desc_layout; SWAPCHAIN_LEN as usize];
        let result = device.fp_v1_0().allocate_descriptor_sets(
            device.handle(),
            &vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(target_img_desc_pool)
                .set_layouts(&set_layouts)
                .build(),
            target_img_descs.as_mut_ptr(),
        );
        assert_eq!(result, vk::Result::SUCCESS);

        let mut swapchain_images: [MaybeUninit<SwapchainImage>; SWAPCHAIN_LEN as usize] =
            MaybeUninit::uninit_array();
        for (i, image) in swapchain_images.iter_mut().enumerate() {
            image.write(SwapchainImage {
                view: vk::ImageView::null(),
                image: vk::Image::null(),
                fence: vk::Fence::null(),
                command_buffer: command_buffers[i],
                image_desc_set: target_img_descs[i],
            });
        }

        RenderState {
            current_frame: 0,
            frames_in_flight: std::mem::transmute(frames_in_flight),
            swapchain_images: std::mem::transmute(swapchain_images),
            swapchain_images_desc_set_layout: target_img_desc_layout,
            format: vk::Format::default(),
            extent: vk::Extent2D::default(),
            swapchain: vk::SwapchainKHR::null(),
            command_pool,
        }
    }
    pub(super) unsafe fn destroy_swapchain(
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
    pub(super) unsafe fn reset_commands(&self, device: &ash::Device) {
        device
            .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())
            .unwrap();
    }
    pub(super) unsafe fn build_swapchain(
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
            device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::builder()
                    .dst_set(swapchain_image.image_desc_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                    .image_info(&[vk::DescriptorImageInfo {
                        sampler: vk::Sampler::null(),
                        image_view: swapchain_image.view,
                        image_layout: vk::ImageLayout::GENERAL, // TODO: ???
                    }])
                    .build()],
                &[],
            );
        }
    }
}
