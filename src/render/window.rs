use super::swapchain::SwapchainImage;
use ash::vk;
use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy::window::{WindowId, Windows};
use std::mem::MaybeUninit;
use std::ops::DerefMut;

use super::swapchain::SurfaceState;
pub const NUM_FRAMES_IN_FLIGHT: u32 = 3;

/// A global resource.
pub struct RenderState {
    pub windows: HashMap<WindowId, ExtractedWindow>,

    pub current_frame: u32,
    pub frames_in_flight: [Frame; NUM_FRAMES_IN_FLIGHT as usize],

    // The command pool for per-frame rendering commands. NUM_FRAMES_IN_FLIGHT commands will be allocated from this.
    pub command_pool: vk::CommandPool,

    pub per_window_desc_set_layout: vk::DescriptorSetLayout,
}

impl RenderState {
    pub unsafe fn new(device: &ash::Device, queues: &crate::Queues) -> Self {
        let mut frames_in_flight: [MaybeUninit<Frame>; NUM_FRAMES_IN_FLIGHT as usize] =
            MaybeUninit::uninit_array();

        let command_pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::builder()
                    .queue_family_index(queues.graphics_queue_family)
                    .flags(
                        vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER
                            | vk::CommandPoolCreateFlags::TRANSIENT,
                    )
                    .build(),
                None,
            )
            .unwrap();

        let mut command_buffers = [vk::CommandBuffer::null(); NUM_FRAMES_IN_FLIGHT as usize];

        let result = device.fp_v1_0().allocate_command_buffers(
            device.handle(),
            &vk::CommandBufferAllocateInfo::builder()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(NUM_FRAMES_IN_FLIGHT)
                .build(),
            command_buffers.as_mut_ptr(),
        );
        assert_eq!(result, vk::Result::SUCCESS);
        for (i, frame) in frames_in_flight.iter_mut().enumerate() {
            frame.write(Frame {
                index: i as u32,
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
                command_buffer: command_buffers[i],
            });
        }
        let per_window_desc_set_layout = device
            .create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::builder()
                    .flags(vk::DescriptorSetLayoutCreateFlags::empty())
                    .bindings(&[
                        vk::DescriptorSetLayoutBinding::builder()
                            .binding(0)
                            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                            .descriptor_count(1)
                            .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
                            .build(),
                        vk::DescriptorSetLayoutBinding::builder()
                            .binding(2)
                            .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                            .descriptor_count(1)
                            .stage_flags(
                                vk::ShaderStageFlags::RAYGEN_KHR
                                    | vk::ShaderStageFlags::CLOSEST_HIT_KHR,
                            )
                            .build(),
                        vk::DescriptorSetLayoutBinding::builder()
                            .binding(3)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .descriptor_count(1)
                            .stage_flags(vk::ShaderStageFlags::INTERSECTION_KHR)
                            .build(),
                    ])
                    .build(),
                None,
            )
            .unwrap();
        Self {
            windows: HashMap::default(),
            current_frame: 0,
            frames_in_flight: std::mem::transmute(frames_in_flight),
            command_pool,
            per_window_desc_set_layout,
        }
    }

    pub fn current_frame(&self) -> &Frame {
        &self.frames_in_flight[self.current_frame as usize]
    }
}

impl FromWorld for RenderState {
    fn from_world(world: &mut World) -> Self {
        let device = world.get_resource::<ash::Device>().unwrap();
        let queues = world.get_resource::<crate::Queues>().unwrap();
        unsafe { Self::new(device, queues) }
    }
}

#[derive(Clone)]
pub struct Frame {
    pub(crate) index: u32,
    pub(crate) render_finished_semaphore: vk::Semaphore,
    pub(crate) fence: vk::Fence,
    pub(crate) command_buffer: vk::CommandBuffer,
}

pub struct ExtractedWindow {
    pub id: WindowId,
    pub handle: bevy::window::RawWindowHandleWrapper,
    pub physical_width: u32,
    pub physical_height: u32,
    pub vsync: bool,

    // Each frame this will be filled with Some. The user should leave this as None after taking its content.
    pub swapchain_image: Option<SwapchainImage>,
    pub size_changed: bool,
    pub state: Option<SurfaceState>,
}

// Token to ensure a system runs on the main thread.
#[derive(Default)]
pub struct NonSendMarker;

#[derive(Default)]
pub struct WindowRenderPlugin;

impl Plugin for WindowRenderPlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app(super::RenderApp);
        render_app
            .init_resource::<RenderState>()
            .init_resource::<NonSendMarker>()
            .add_system_to_stage(super::RenderStage::Extract, extract_windows)
            .add_system_to_stage(super::RenderStage::Prepare, prepare_windows)
            .add_system_to_stage(super::RenderStage::Cleanup, switch_to_next_frame);
    }
}

fn extract_windows(mut render_world: ResMut<super::RenderWorld>, windows: Res<Windows>) {
    let mut render_state = render_world.get_resource_mut::<RenderState>().unwrap();
    for window in windows.iter() {
        let (new_width, new_height) = (
            window.physical_width().max(1),
            window.physical_height().max(1),
        );

        let mut extracted_window =
            render_state
                .windows
                .entry(window.id())
                .or_insert(ExtractedWindow {
                    id: window.id(),
                    handle: window.raw_window_handle(),
                    physical_width: new_width,
                    physical_height: new_height,
                    vsync: window.vsync(),
                    swapchain_image: None,
                    size_changed: false,
                    state: None,
                });

        // NOTE: Drop the swap chain frame here
        extracted_window.swapchain_image = None;
        extracted_window.size_changed = new_width != extracted_window.physical_width
            || new_height != extracted_window.physical_height;
        extracted_window.physical_width = new_width;
        extracted_window.physical_height = new_height;
    }
}

pub fn prepare_windows(
    // By accessing a NonSend resource, we tell the scheduler to put this system on the main thread,
    // which is necessary for some OS s
    _marker: NonSend<NonSendMarker>,
    mut render_state: ResMut<RenderState>,
    entry: Res<ash::Entry>,
    device: Res<ash::Device>,
    instance: Res<ash::Instance>,
    surface_loader: Res<ash::extensions::khr::Surface>,
    swapchain_loader: Res<ash::extensions::khr::Swapchain>,
    physical_device: Res<vk::PhysicalDevice>,
    queues: Res<crate::Queues>,
) {
    let render_state = render_state.deref_mut();
    let frame_in_flight = render_state.current_frame().clone();

    unsafe {
        device
            // Wait so that the previous frame finishes rendering
            // TODO: maybe move to after acquire the image?
            .wait_for_fences(&[frame_in_flight.fence], true, u64::MAX)
            .unwrap();
    }

    for window in render_state.windows.values_mut() {
        let surface_state = match &mut window.state {
            Some(state) => unsafe {
                if window.size_changed {
                    state.destroy_swapchain(&device, &swapchain_loader);
                    state.build_swapchain(
                        render_state.per_window_desc_set_layout,
                        &instance,
                        &device,
                        &surface_loader,
                        &swapchain_loader,
                        *physical_device,
                        &queues,
                    );
                }
                state
            },
            None => unsafe {
                let mut state = SurfaceState::new(
                    &entry,
                    &instance,
                    &device,
                    *physical_device,
                    &surface_loader,
                    &window.handle,
                );
                state.build_swapchain(
                    render_state.per_window_desc_set_layout,
                    &instance,
                    &device,
                    &surface_loader,
                    &swapchain_loader,
                    *physical_device,
                    &queues,
                );
                window.state = Some(state);
                window.state.as_mut().unwrap()
            },
        };

        let swapchain_image =
            unsafe { surface_state.next_image(&device, &frame_in_flight, &swapchain_loader) };

        unsafe {
            device.reset_fences(&[frame_in_flight.fence]).unwrap();
        }
        window.swapchain_image = Some(swapchain_image)
    }
}

pub fn render_system(world: &mut bevy::ecs::world::World) {
    let (mut render_state, device, swapchain_loader, queues) = SystemState::<(
        ResMut<RenderState>,
        Res<ash::Device>,
        Res<ash::extensions::khr::Swapchain>,
        Res<crate::Queues>,
    )>::new(world)
    .get_mut(world);

    let current_frame = render_state.current_frame().clone();

    for window in render_state.windows.values_mut() {
        if window.state.is_none() {
            println!("Surface not initiallized... skipped");
        }
        // Per-window state with information regarding the current window
        let surface_state = window.state.as_ref().unwrap();

        // Per-window image obtained from the prepare stage with current swapchain frame information.
        let swapchain_image = window
            .swapchain_image
            .take()
            .expect("The swapchain texture was never generated or already consumed.");
        let swapchain_image_available_semaphore =
            surface_state.image_available_semaphore[current_frame.index as usize];
        unsafe {
            // TODO: If we were to support multiple windows in the future, we can potentially batch submissions here.
            device
                .queue_submit(
                    queues.graphics_queue,
                    &[vk::SubmitInfo::builder()
                        .wait_semaphores(&[swapchain_image_available_semaphore]) // swapchain image available semaphore
                        // Wait for swapchain image to become available before starting ray tracing
                        .wait_dst_stage_mask(&[vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR])
                        .command_buffers(&[current_frame.command_buffer])
                        .signal_semaphores(&[current_frame.render_finished_semaphore])
                        .build()],
                    current_frame.fence,
                )
                .unwrap();

            let suboptimal = swapchain_loader
                .queue_present(
                    queues.graphics_queue,
                    &vk::PresentInfoKHR::builder()
                        .wait_semaphores(&[current_frame.render_finished_semaphore])
                        .swapchains(&[surface_state.swapchain])
                        .image_indices(&[swapchain_image.index])
                        .build(),
                )
                .unwrap();

            if suboptimal {
                println!("Suboptimal~!!!");
            }
        }
    }
}

fn switch_to_next_frame(mut render_state: ResMut<RenderState>) {
    for window in render_state.windows.values() {
        assert!(
            window.swapchain_image.is_none(),
            "The frame needs to be consumed by the render system."
        );
    }

    render_state.current_frame = render_state.current_frame + 1;
    if render_state.current_frame >= NUM_FRAMES_IN_FLIGHT {
        render_state.current_frame = 0;
    }
}
