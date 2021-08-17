use ash::vk;
use std::ffi::c_void;
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    supported_extensions: Vec<vk::ExtensionProperties>,
    pub physical_device_properties: vk::PhysicalDeviceProperties,
    pub acceleration_structure_properties: vk::PhysicalDeviceAccelerationStructurePropertiesKHR,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    // TODO: use proc macro to generate bitfields for this
    pub features: vk::PhysicalDeviceFeatures,
    pub buffer_device_address_features: vk::PhysicalDeviceBufferDeviceAddressFeatures,
    pub acceleration_structure_features: vk::PhysicalDeviceAccelerationStructureFeaturesKHR,
}
unsafe impl Send for DeviceInfo {}
unsafe impl Sync for DeviceInfo {}

impl DeviceInfo {
    pub unsafe fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
    ) -> Self {
        let mut features = vk::PhysicalDeviceFeatures2::default();
        let mut buffer_device_address_features =
            vk::PhysicalDeviceBufferDeviceAddressFeatures::default();
        let mut acceleration_structure_features =
            vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default();
        features.p_next = &mut buffer_device_address_features as *mut _ as *mut c_void;
        buffer_device_address_features.p_next =
            &mut acceleration_structure_features as *mut _ as *mut c_void;
        instance.get_physical_device_features2(physical_device, &mut features);

        let mut properties2 = vk::PhysicalDeviceProperties2::default();
        let mut acceleration_structure_properties =
            vk::PhysicalDeviceAccelerationStructurePropertiesKHR::default();
        properties2.p_next = &mut acceleration_structure_properties as *mut _ as *mut c_void;
        instance.get_physical_device_properties2(physical_device, &mut properties2);

        Self {
            supported_extensions: entry.enumerate_instance_extension_properties().unwrap(),
            physical_device_properties: properties2.properties,
            acceleration_structure_properties: acceleration_structure_properties,
            memory_properties: instance.get_physical_device_memory_properties(physical_device),
            features: features.features,
            buffer_device_address_features,
            acceleration_structure_features,
        }
    }

    pub fn memory_heaps(&self) -> &[vk::MemoryHeap] {
        &self.memory_properties.memory_heaps[..self.memory_properties.memory_heap_count as usize]
    }

    pub fn memory_types(&self) -> &[vk::MemoryType] {
        &self.memory_properties.memory_types[..self.memory_properties.memory_type_count as usize]
    }
}
