use crate::swapchain::PresentationFrame;
use ash::vk;
use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use bevy::render2::view::{ExtractedWindow, ExtractedWindows};
/// This WindowRenderPlugin replaces bevy's window plugin so that we can take over swapchain creation.
use bevy::render2::{RenderApp, RenderStage, RenderWorld};
use bevy::utils::HashMap;
use bevy::window::{WindowId, Windows};
use std::ops::DerefMut;

use super::swapchain::SurfaceState;

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
            .init_resource::<WindowSurfaces>()
            .init_resource::<NonSendMarker>()
            .add_system_to_stage(RenderStage::Extract, extract_windows)
            .add_system_to_stage(RenderStage::Prepare, prepare_windows);
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
    pub next_frame: Option<PresentationFrame>,
}

#[derive(Default)]
pub struct WindowSurfaces {
    pub surfaces: HashMap<WindowId, WindowSurface>,
}

pub fn prepare_windows(
    // By accessing a NonSend resource, we tell the scheduler to put this system on the main thread,
    // which is necessary for some OS s
    _marker: NonSend<NonSendMarker>,
    mut windows: ResMut<ExtractedWindows>,
    mut window_surfaces: ResMut<WindowSurfaces>,
    entry: Res<ash::Entry>,
    device: Res<ash::Device>,
    instance: Res<ash::Instance>,
    surface_loader: Res<ash::extensions::khr::Surface>,
    swapchain_loader: Res<ash::extensions::khr::Swapchain>,
    physical_device: Res<vk::PhysicalDevice>,
    wgpu_device: Res<bevy::render2::renderer::RenderDevice>,
) {
    println!("prepare windows");
    let window_surfaces = window_surfaces.deref_mut();
    for window in windows.windows.values_mut() {
        use std::collections::hash_map::Entry;
        let window_surface = match window_surfaces.surfaces.entry(window.id) {
            Entry::Occupied(entry) => {
                let window_surface = entry.into_mut();
                if window.size_changed {
                    unsafe {
                        window_surface
                            .state
                            .destroy_swapchain(&device, &swapchain_loader);
                        window_surface.state.build_swapchain(
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

        let presentation_frame =
            unsafe { window_surface.state.next_frame(&device, &swapchain_loader) };

        window.swap_chain_texture = Some(presentation_frame.swapchain_image.bevy_texture.clone());
        window_surface.next_frame = Some(presentation_frame)
    }
}
