mod commands;
mod raytracer;
mod state;

use state::RenderState;

use ash::vk;
use bevy::prelude::*;

use crate::Queues;

#[derive(Default)]
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SwapchainRebuilt>()
            .add_startup_system(setup)
            .add_startup_system_to_stage(StartupStage::PostStartup, raytracer::raytracing_setup)
            .add_system(rebuild.label("rebuild swapchain on resize"))
            .add_system(
                commands::record_command_buffers_system
                    .label("rebuild command buffers")
                    .after("rebuild swapchain on resize"),
            )
            .add_system(update.after("rebuild command buffers"));
    }
}

fn setup(
    mut commands: Commands,
    mut swapchain_rebuilt_events: EventWriter<SwapchainRebuilt>,
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
        swapchain_rebuilt_events.send(SwapchainRebuilt);
        commands.insert_resource(swapchain_loader);
        commands.insert_resource(render_state);
    }
}

pub struct SwapchainRebuilt;

pub(super) fn rebuild(
    mut window_resized_events: EventReader<bevy::window::WindowResized>,
    mut swapchain_rebuilt_events: EventWriter<SwapchainRebuilt>,
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
    }
    swapchain_rebuilt_events.send(SwapchainRebuilt);
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
