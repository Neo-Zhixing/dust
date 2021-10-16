use crate::swapchain::{SwapchainImage};
use ash::vk;
use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use bevy::render2::view::{ExtractedWindow, ExtractedWindows};
/// This WindowRenderPlugin replaces bevy's window plugin so that we can take over swapchain creation.
use bevy::render2::{RenderApp, RenderStage, RenderWorld};
use bevy::utils::HashMap;
use bevy::window::{WindowId, Windows};
use std::mem::MaybeUninit;
use std::ops::DerefMut;

use super::swapchain::SurfaceState;
pub const NUM_FRAMES_IN_FLIGHT: u32 = 3;

// Token to ensure a system runs on the main thread.
#[derive(Default)]
pub struct NonSendMarker;

#[derive(Default)]
pub struct WindowRenderPlugin;

impl Plugin for WindowRenderPlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app(RenderApp);

        let instance = render_app.world.get_resource::<ash::Instance>().unwrap();
        let device = render_app.world.get_resource::<ash::Device>().unwrap();
        let entry = render_app.world.get_resource::<ash::Entry>().unwrap();
        let swapchain_loader = ash::extensions::khr::Swapchain::new(instance, device);
        let surface_loader = ash::extensions::khr::Surface::new(entry, instance);

        render_app
            .insert_resource(swapchain_loader)
            .insert_resource(surface_loader)
            .init_resource::<ExtractedWindows>()
            .init_resource::<RenderState>()
            .init_resource::<NonSendMarker>()
            .add_system_to_stage(RenderStage::Extract, extract_windows)
            .add_system_to_stage(RenderStage::Prepare, prepare_windows)
            .add_system_to_stage(RenderStage::Cleanup, switch_to_next_frame);
    }
}

fn extract_windows(mut render_world: ResMut<RenderWorld>, windows: Res<Windows>) {
    let mut extracted_windows = render_world.get_resource_mut::<ExtractedWindows>().unwrap();
    for window in windows.iter() {
        let (new_width, new_height) = (
            window.physical_width().max(1),
            window.physical_height().max(1),
        );

        let mut extracted_window =
            extracted_windows
                .entry(window.id())
                .or_insert(ExtractedWindow {
                    id: window.id(),
                    handle: window.raw_window_handle(),
                    physical_width: new_width,
                    physical_height: new_height,
                    vsync: window.vsync(),
                    swap_chain_texture: None,
                    size_changed: false,
                });

        // NOTE: Drop the swap chain frame here
        extracted_window.swap_chain_texture = None;
        extracted_window.size_changed = new_width != extracted_window.physical_width
            || new_height != extracted_window.physical_height;
        extracted_window.physical_width = new_width;
        extracted_window.physical_height = new_height;
    }
}

pub struct WindowSurface {
    pub state: SurfaceState,
    pub next_frame: Option<(SwapchainImage, u32)>,
}

#[derive(Clone)]
pub struct Frame {
    pub(crate) index: u32,
    pub(crate) render_finished_semaphore: vk::Semaphore,
    pub(crate) fence: vk::Fence,
    pub(crate) command_buffer: vk::CommandBuffer,
}


pub struct RenderState {
    pub surfaces: HashMap<WindowId, WindowSurface>,
    
    current_frame: u32,
    frames_in_flight: [Frame; NUM_FRAMES_IN_FLIGHT as usize],

    // The command pool for per-frame rendering commands. NUM_FRAMES_IN_FLIGHT commands will be allocated from this.
    command_pool: vk::CommandPool,
}

impl FromWorld for RenderState {
    fn from_world(world: &mut World) -> Self {
        let device = world.get_resource::<ash::Device>().unwrap();
        let queues = world.get_resource::<crate::Queues>().unwrap();
        unsafe {
            Self::new(device, queues)
        }
    }
}

impl RenderState {
    pub unsafe fn new(device: &ash::Device, queues: &crate::Queues) -> Self {
        let mut frames_in_flight: [MaybeUninit<Frame>; NUM_FRAMES_IN_FLIGHT as usize] =
        MaybeUninit::uninit_array();
        
        let command_pool = device.create_command_pool(
            &vk::CommandPoolCreateInfo::builder()
            .queue_family_index(queues.graphics_queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER | vk::CommandPoolCreateFlags::TRANSIENT)
            .build(),
            None,
        ).unwrap();

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
        Self {
            surfaces: HashMap::default(),
            current_frame: 0,
            frames_in_flight: std::mem::transmute(frames_in_flight),
            command_pool,
        }
    }
    
    pub fn current_frame(&self) -> &Frame {
        &self.frames_in_flight[self.current_frame as usize]
    }
}


pub fn prepare_windows(
    // By accessing a NonSend resource, we tell the scheduler to put this system on the main thread,
    // which is necessary for some OS s
    _marker: NonSend<NonSendMarker>,
    mut windows: ResMut<ExtractedWindows>,
    mut window_surfaces: ResMut<RenderState>,
    entry: Res<ash::Entry>,
    device: Res<ash::Device>,
    instance: Res<ash::Instance>,
    surface_loader: Res<ash::extensions::khr::Surface>,
    swapchain_loader: Res<ash::extensions::khr::Swapchain>,
    physical_device: Res<vk::PhysicalDevice>,
    wgpu_device: Res<bevy::render2::renderer::RenderDevice>,
) {
    let render_state = window_surfaces.deref_mut();
    let frame_in_flight = render_state.current_frame().clone();
    
    
    unsafe {
        device
        // Wait so that the previous frame finishes rendering
        // TODO: maybe move to after acquire the image?
        .wait_for_fences(&[frame_in_flight.fence], true, u64::MAX)
        .unwrap();
    }





    for window in windows.windows.values_mut() {
        use std::collections::hash_map::Entry;
        let window_surface = match render_state.surfaces.entry(window.id) {
            Entry::Occupied(entry) => {
                let window_surface = entry.into_mut();
                if window.size_changed {
                    unsafe {
                        window_surface
                            .state
                            .destroy_swapchain(&device, &swapchain_loader);
                        window_surface.state.build_swapchain(
                            &instance,
                            &surface_loader,
                            &swapchain_loader,
                            *physical_device,
                            wgpu_device.wgpu_device(),
                        );
                    }
                }
                window_surface
            }
            Entry::Vacant(vacant_entry) => {
                let state = unsafe {
                    let mut state = SurfaceState::new(&entry, &instance, &device, &window.handle);
                    state.build_swapchain(
                        &instance,
                        &surface_loader,
                        &swapchain_loader,
                        *physical_device,
                        wgpu_device.wgpu_device(),
                    );
                    state
                };
                vacant_entry.insert(WindowSurface {
                    state,
                    next_frame: None,
                })
            }
        };

        let (swapchain_image, swapchain_image_index) =
            unsafe { window_surface.state.next_image(&device, &frame_in_flight, &swapchain_loader) };

        unsafe {
            device.reset_fences(&[frame_in_flight.fence]).unwrap();
        }
        
        window.swap_chain_texture = Some(swapchain_image.bevy_texture.clone());
        window_surface.next_frame = Some((swapchain_image, swapchain_image_index))
    }
    

}

fn switch_to_next_frame(
    mut render_state: ResMut<RenderState>,
    device: Res<ash::Device>,
) {
    render_state.current_frame = render_state.current_frame + 1;
    if render_state.current_frame >= NUM_FRAMES_IN_FLIGHT {
        render_state.current_frame = 0;
    }
}