use ash::vk;

pub struct Queues {
    pub graphics_queue_family: u32,
    pub graphics_queue: vk::Queue,
    pub transfer_binding_queue_family: u32,
    pub transfer_binding_queue: vk::Queue,
    pub compute_queue: vk::Queue,
    pub compute_queue_family: u32,
}

impl Queues {
    pub(crate) unsafe fn new(
        device: &ash::Device,
        graphics_queue_family: u32,
        compute_queue_family: u32,
        transfer_binding_queue_family: u32,
    ) -> Queues {
        let graphics_queue = device.get_device_queue(graphics_queue_family, 0);
        let compute_queue = device.get_device_queue(compute_queue_family, 0);
        let transfer_binding_queue = device.get_device_queue(transfer_binding_queue_family, 0);
        Queues {
            graphics_queue,
            graphics_queue_family,
            compute_queue,
            compute_queue_family,
            transfer_binding_queue,
            transfer_binding_queue_family,
        }
    }
}
