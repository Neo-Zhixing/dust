use ash::vk;

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    supported_extensions: Vec<vk::ExtensionProperties>,
    pub physical_device_properties: vk::PhysicalDeviceProperties,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    // TODO: use proc macro to generate bitfields for this
    pub features: vk::PhysicalDeviceFeatures,
    pub buffer_device_address_features: vk::PhysicalDeviceBufferDeviceAddressFeatures,
}
unsafe impl Send for DeviceInfo{}
unsafe impl Sync for DeviceInfo{}

impl DeviceInfo {
    pub unsafe fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
    ) -> Self {
        let mut features = vk::PhysicalDeviceFeatures2::default();
        let mut buffer_device_address_features = vk::PhysicalDeviceBufferDeviceAddressFeatures::default();
        features.p_next =
            &mut buffer_device_address_features as *mut vk::PhysicalDeviceBufferDeviceAddressFeatures as *mut _;
        instance.get_physical_device_features2(physical_device, &mut features);

        Self {
            supported_extensions: entry.enumerate_instance_extension_properties().unwrap(),
            physical_device_properties: instance.get_physical_device_properties(physical_device),
            memory_properties: instance.get_physical_device_memory_properties(physical_device),
            features: features.features,
            buffer_device_address_features
        }
    }

    pub fn memory_heaps(&self) -> &[vk::MemoryHeap] {
        &self.memory_properties.memory_heaps[..self.memory_properties.memory_heap_count as usize]
    }

    pub fn memory_types(&self) -> &[vk::MemoryType] {
        &self.memory_properties.memory_types[..self.memory_properties.memory_type_count as usize]
    }
}
