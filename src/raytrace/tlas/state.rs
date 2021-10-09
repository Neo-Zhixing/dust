use ash::vk;
use gpu_alloc_ash::AshMemoryDevice;

pub struct TlasState {
    pub tlas: vk::AccelerationStructureKHR,
    pub(super) tlas_buf: vk::Buffer,
    pub(super) tlas_mem: Option<crate::MemoryBlock>,
    pub(super) tlas_data_buf: vk::Buffer,
    pub(super) tlas_data_mem: Option<crate::MemoryBlock>,
    pub(super) tlas_scratch_buf: vk::Buffer,
    pub(super) tlas_scratch_mem: Option<crate::MemoryBlock>,
    pub(super) unit_box_as: vk::AccelerationStructureKHR,
    pub(super) unit_box_as_buf: vk::Buffer,
    pub(super) unit_box_as_mem: crate::MemoryBlock,
    pub(super) unit_box_as_device_address: u64,
    pub(super) unit_box_scratch_mem: Option<crate::MemoryBlock>,
    pub(super) unit_box_scratch_buf: vk::Buffer,
    pub(super)command_pool: vk::CommandPool,
    pub(super) command_buffer: vk::CommandBuffer,
    pub(super)needs_update_next_frame: bool,
    pub(super)have_updates_pending: bool,
    pub fence: vk::Fence,
}

impl TlasState {
    /// Should we actually perform the updates this frame.
    /// have_updates_this_frame: true if anything changes this frame and tlas needs update.
    /// returns: true if we actually need to rebuild the tlas this frame.
    /// This serves as a buffer so that if an tlas update takes more than a frame, subsequent updates will be deferred to the next frames
    pub fn should_update(&mut self, device: &ash::Device, allocator: &mut crate::Allocator, have_updates_this_frame: bool) -> bool {
        let mut have_updates_pending = self.have_updates_pending;
        if have_updates_pending {
            let updates_finished = unsafe { device.get_fence_status(self.fence).unwrap() };
            if updates_finished {
                unsafe {
                    device.reset_fences(&[self.fence]).unwrap();
                    self.have_updates_pending = false;
                    have_updates_pending = false;
                    device
                        .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())
                        .unwrap();
    
                    // Cleanup for Unit Box BLAS
                    if self.unit_box_scratch_buf != vk::Buffer::null() {
                        device.destroy_buffer(self.unit_box_scratch_buf, None);
                        self.unit_box_scratch_buf = vk::Buffer::null();
                    }
                    if let Some(mem) = self.unit_box_scratch_mem.take() {
                        allocator.dealloc(AshMemoryDevice::wrap(&*device), mem);
                    }
    
                    // Cleanup for TLAS
                    if self.tlas_data_buf != vk::Buffer::null() {
                        device.destroy_buffer(self.tlas_data_buf, None);
                        self.tlas_data_buf = vk::Buffer::null();
                    }
                    if let Some(mem) = self.tlas_data_mem.take() {
                        allocator.dealloc(AshMemoryDevice::wrap(&*device), mem);
                    }
                    if self.tlas_scratch_buf != vk::Buffer::null() {
                        device.destroy_buffer(self.tlas_scratch_buf, None);
                        self.tlas_scratch_buf = vk::Buffer::null();
                    }
                    if let Some(mem) = self.tlas_scratch_mem.take() {
                        allocator.dealloc(AshMemoryDevice::wrap(&*device), mem);
                    }
                }
            }
        }
        let need_to_do_updates = self.needs_update_next_frame | have_updates_this_frame;
        if !need_to_do_updates {
            return false;
        }
    
        if have_updates_pending {
            self.needs_update_next_frame = true;
            // Defer the work to next frame
            return false;
        }
        self.needs_update_next_frame = false;
        return true;
    }
    pub(super) fn did_updates(&mut self) {
        self.have_updates_pending = true;
    }
}