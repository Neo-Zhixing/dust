use bevy::ecs::world::World;
use bevy::render2::{
    camera::{CameraPlugin, ExtractedCamera, ExtractedCameraNames},
    render_graph::{Node, NodeRunError, RenderGraphContext, SlotValue},
    renderer::RenderContext,
    view::ExtractedWindows,
};

use bevy::core_pipeline::ViewDepthTexture;

pub struct RayPassDriverNode;

impl RayPassDriverNode {
    pub const NAME: &'static str = "ray_pass_driver";
}

impl Node for RayPassDriverNode {
    fn run(
        &self,
        graph: &mut RenderGraphContext,
        _render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let extracted_cameras = world.get_resource::<ExtractedCameraNames>().unwrap();
        let extracted_windows = world.get_resource::<ExtractedWindows>().unwrap();

        if let Some(camera_3d) = extracted_cameras.entities.get(CameraPlugin::CAMERA_3D) {
            let extracted_camera = world.entity(*camera_3d).get::<ExtractedCamera>().unwrap();
            let depth_texture = world.entity(*camera_3d).get::<ViewDepthTexture>().unwrap();
            let extracted_window = extracted_windows.get(&extracted_camera.window_id).unwrap();
            let swap_chain_texture = extracted_window.swap_chain_frame.as_ref().unwrap().clone();
            graph.run_sub_graph(
                super::raytracing_graph::NAME,
                vec![
                    SlotValue::Entity(*camera_3d),
                    SlotValue::TextureView(swap_chain_texture),
                    SlotValue::TextureView(depth_texture.view.clone()),
                ],
            )?;
        }

        Ok(())
    }
}
