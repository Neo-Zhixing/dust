use super::window::Frame;
use ash::vk;
use bevy::window::RawWindowHandleWrapper;

const SWAPCHAIN_LEN: u32 = 3;
use super::window::NUM_FRAMES_IN_FLIGHT;

#[derive(Clone)]
pub struct SwapchainImage {
    pub index: u32,
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub fence: vk::Fence, // This fence was borrowed from the last rendered frame.
    // The reason we need a separate command buffer for each swapchain image
    // is that cmd_begin_render_pass contains a reference to the framebuffer
    // which is unique to each swapchain image.
    pub desc_set: vk::DescriptorSet, // A desc set that binds to the target image.
}

pub struct SurfaceState {
    // This is per-window
    pub surface: vk::SurfaceKHR,
    pub swapchain: vk::SwapchainKHR,
    pub desc_pool: vk::DescriptorPool, // Desc pool for storing swapchain related desc sets
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub swapchain_images: Vec<SwapchainImage>,
    pub image_available_semaphore: [vk::Semaphore; NUM_FRAMES_IN_FLIGHT as usize], // This should really be per frame, per window
}

impl SurfaceState {
    pub unsafe fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        device: &ash::Device,
        physical_device: vk::PhysicalDevice,
        surface_loader: &ash::extensions::khr::Surface,
        window_handle: &RawWindowHandleWrapper,
    ) -> Self {
        let window_handle = window_handle.get_handle();
        let surface = ash_window::create_surface(entry, instance, &window_handle, None).unwrap();

        let mut image_available_semaphore = [vk::Semaphore::null(); NUM_FRAMES_IN_FLIGHT as usize];
        for semaphore in image_available_semaphore.iter_mut() {
            *semaphore = device
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                .unwrap();
        }

        let caps = surface_loader
            .get_physical_device_surface_capabilities(physical_device, surface)
            .unwrap();
        let desc_pool = device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::builder()
                    .flags(vk::DescriptorPoolCreateFlags::empty())
                    .max_sets(caps.max_image_count)
                    .pool_sizes(&[vk::DescriptorPoolSize {
                        ty: vk::DescriptorType::STORAGE_IMAGE,
                        descriptor_count: caps.max_image_count, // Swapchain can contain any number of images >= SWAPCHAIN_LEN
                    }])
                    .build(),
                None,
            )
            .unwrap();

        Self {
            surface,
            swapchain: vk::SwapchainKHR::null(),
            format: vk::Format::default(),
            extent: vk::Extent2D::default(),
            swapchain_images: Vec::new(),
            image_available_semaphore,
            desc_pool,
        }
    }
    pub unsafe fn destroy_swapchain(
        &mut self,
        device: &ash::Device,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) {
        device.device_wait_idle().unwrap();
        // We have to reallocate descriptor sets here, because we might have a different number of desc sets in our next frame.
        device
            .reset_descriptor_pool(self.desc_pool, vk::DescriptorPoolResetFlags::empty())
            .unwrap();
        for image in self.swapchain_images.iter() {
            device.destroy_image_view(image.view, None);
        }
        self.swapchain_images.clear();
        swapchain_loader.destroy_swapchain(self.swapchain, None);
        self.swapchain = vk::SwapchainKHR::null();
    }
    pub unsafe fn build_swapchain(
        &mut self,
        per_window_desc_set_layout: vk::DescriptorSetLayout,
        instance: &ash::Instance,
        device: &ash::Device,
        surface_loader: &ash::extensions::khr::Surface,
        swapchain_loader: &ash::extensions::khr::Swapchain,
        physical_device: vk::PhysicalDevice,
        queues: &crate::Queues,
    ) {
        if !surface_loader
            .get_physical_device_surface_support(
                physical_device,
                queues.graphics_queue_family,
                self.surface,
            )
            .unwrap_or(false)
        {
            panic!("The current physical device is incompatible with the surface.");
        }
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
        let desc_set_layouts = vec![per_window_desc_set_layout; images.len()];

        let desc_sets = device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(self.desc_pool)
                    .set_layouts(&desc_set_layouts)
                    .build(),
            )
            .unwrap();

        self.swapchain_images = images
            .iter()
            .enumerate()
            .map(|(i, &image)| {
                let view = device
                    .create_image_view(
                        &vk::ImageViewCreateInfo::builder()
                            .flags(vk::ImageViewCreateFlags::empty())
                            .image(image)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(self.format)
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
                            .build(),
                        None,
                    )
                    .unwrap();
                SwapchainImage {
                    index: i as u32,
                    image,
                    view,
                    fence: vk::Fence::null(),
                    desc_set: desc_sets[i],
                }
            })
            .collect();

        let image_infos: Vec<vk::DescriptorImageInfo> = self
            .swapchain_images
            .iter()
            .map(|image| vk::DescriptorImageInfo {
                sampler: vk::Sampler::null(),
                image_layout: vk::ImageLayout::GENERAL,
                image_view: image.view,
            })
            .collect();
        let desc_writes: Vec<vk::WriteDescriptorSet> = self
            .swapchain_images
            .iter()
            .enumerate()
            .map(|(i, image)| {
                vk::WriteDescriptorSet::builder()
                    .dst_set(image.desc_set)
                    .dst_binding(0)
                    .dst_array_element(0)
                    .image_info(&image_infos[i..=i])
                    .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                    .build()
            })
            .collect();
        device.update_descriptor_sets(&desc_writes, &[]);
    }

    pub unsafe fn next_image(
        &mut self,
        device: &ash::Device,
        frame_in_flight: &Frame,
        swapchain_loader: &ash::extensions::khr::Swapchain,
    ) -> SwapchainImage {
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
            if swapchain_image.fence != vk::Fence::null()
                && swapchain_image.fence != frame_in_flight.fence
            {
                // Make sure that the previous frame using the current swapchain image finishes rendering
                device
                    .wait_for_fences(&[swapchain_image.fence], true, u64::MAX)
                    .unwrap();
            }
            swapchain_image.fence = frame_in_flight.fence;
        }
        assert_eq!(swapchain_image.index, image_index);
        swapchain_image.clone()
    }
}
