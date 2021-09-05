mod ray_pass_driver;

use bevy::prelude::*;
use bevy::render2::render_graph::{Node, RenderGraph, SlotInfo, SlotType};
use bevy::render2::RenderApp;

#[derive(Default)]
pub struct RaytracingPipelinePlugin;

mod raytracing_graph {
    pub const NAME: &str = "ray_pass";
    pub mod input {
        pub const VIEW_ENTITY: &str = "view_entity";
        pub const RENDER_TARGET: &str = "render_target";
        pub const DEPTH: &str = "depth";
    }
}

impl Plugin for RaytracingPipelinePlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app(RenderApp);

        let raytracing_node = RaytracingNode::new(&mut render_app.world);

        let mut raytracing_graph = RenderGraph::default();
        raytracing_graph.add_node(RaytracingNode::NAME, raytracing_node);

        let input_node_id = raytracing_graph.set_input(vec![
            SlotInfo::new(raytracing_graph::input::VIEW_ENTITY, SlotType::Entity),
            SlotInfo::new(
                raytracing_graph::input::RENDER_TARGET,
                SlotType::TextureView,
            ),
            SlotInfo::new(raytracing_graph::input::DEPTH, SlotType::TextureView),
        ]);

        raytracing_graph
            .add_slot_edge(
                input_node_id,
                raytracing_graph::input::RENDER_TARGET,
                RaytracingNode::NAME,
                RaytracingNode::IN_COLOR_ATTACHMENT,
            )
            .unwrap();
        raytracing_graph
            .add_slot_edge(
                input_node_id,
                raytracing_graph::input::DEPTH,
                RaytracingNode::NAME,
                RaytracingNode::IN_DEPTH,
            )
            .unwrap();
        raytracing_graph
            .add_slot_edge(
                input_node_id,
                raytracing_graph::input::VIEW_ENTITY,
                RaytracingNode::NAME,
                RaytracingNode::IN_VIEW,
            )
            .unwrap();

        let mut graph = render_app.world.get_resource_mut::<RenderGraph>().unwrap();
        graph.add_sub_graph(raytracing_graph::NAME, raytracing_graph);
        graph.add_node(
            ray_pass_driver::RayPassDriverNode::NAME,
            ray_pass_driver::RayPassDriverNode,
        );
    }
}

pub struct RaytracingNode {}

impl RaytracingNode {
    const NAME: &'static str = "main_pass";
    pub const IN_COLOR_ATTACHMENT: &'static str = "color_attachment";
    pub const IN_DEPTH: &'static str = "depth";
    pub const IN_VIEW: &'static str = "view";
    pub fn new(world: &mut World) -> Self {
        RaytracingNode {}
    }
}

impl Node for RaytracingNode {
    fn input(&self) -> Vec<SlotInfo> {
        vec![
            SlotInfo::new(Self::IN_COLOR_ATTACHMENT, SlotType::TextureView),
            SlotInfo::new(Self::IN_DEPTH, SlotType::TextureView),
            SlotInfo::new(Self::IN_VIEW, SlotType::Entity),
        ]
    }

    fn run(
        &self,
        graph: &mut bevy::render2::render_graph::RenderGraphContext,
        render_context: &mut bevy::render2::renderer::RenderContext,
        world: &World,
    ) -> Result<(), bevy::render2::render_graph::NodeRunError> {
        let device = world.get_resource::<ash::Device>().unwrap();
        Ok(())
    }
}
