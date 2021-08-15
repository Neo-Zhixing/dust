use ash::vk;

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub supported_extensions: Vec<vk::ExtensionProperties>,
    pub physical_device_properties: vk::PhysicalDeviceProperties,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub features: vk::PhysicalDeviceFeatures,
}

impl DeviceInfo {
    pub unsafe fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
    ) -> Self {
        Self {
            supported_extensions: entry.enumerate_instance_extension_properties().unwrap(),
            physical_device_properties: instance.get_physical_device_properties(physical_device),
            memory_properties: instance.get_physical_device_memory_properties(physical_device),
            features: instance.get_physical_device_features(physical_device),
        }
    }
}
