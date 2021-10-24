mod state;
mod uniform;
pub use state::TlasState;
use std::mem::MaybeUninit;
pub use uniform::UniformArray;

use ash::vk;
use bevy::{ecs::system::SystemState, prelude::*, utils::HashMap};

use crate::render::{RenderStage, RenderWorld};
use gpu_alloc_ash::AshMemoryDevice;

use crate::{device_info::DeviceInfo, raytrace::tlas::uniform::UniformEntry, Queues, VoxelModel};
#[derive(Debug)]
pub struct Raytraced {
    pub aabb_extent: bevy::math::Vec3,
}
#[derive(Default)]
pub struct TlasPlugin;

impl Plugin for TlasPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(uniform::UniformArray::new());
        tlas_setup(app);
        app.add_system_to_stage(RenderStage::Extract, tlas_update);
        //.add_system_to_stage(
        //    CoreStage::PostUpdate,
        //    tlas_update.after(bevy::transform::TransformSystem::TransformPropagate),
        //);
    }
}

fn tlas_setup(app: &mut App) {
    let (device, mut allocator, queues, device_info, acceleration_structure_loader) =
        SystemState::<(
            Res<ash::Device>,
            ResMut<crate::Allocator>,
            Res<Queues>,
            Res<DeviceInfo>,
            Res<ash::extensions::khr::AccelerationStructure>,
        )>::new(&mut app.world)
        .get_mut(&mut app.world);

    unsafe {
        let command_pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::builder()
                    .flags(vk::CommandPoolCreateFlags::TRANSIENT)
                    .queue_family_index(queues.compute_queue_family)
                    .build(),
                None,
            )
            .unwrap();
        let command_buffer = {
            let mut command_buffer = vk::CommandBuffer::null();
            let result = device.fp_v1_0().allocate_command_buffers(
                device.handle(),
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1)
                    .build() as *const _,
                &mut command_buffer,
            );
            assert_eq!(result, vk::Result::SUCCESS);
            command_buffer
        };

        let unit_box = vk::AabbPositionsKHR {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 1.0,
            max_y: 1.0,
            max_z: 1.0,
        };
        let unit_box_buffer = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .size(std::mem::size_of_val(&unit_box) as u64)
                    .usage(
                        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let unit_box_requirements = device.get_buffer_memory_requirements(unit_box_buffer);
        let mut unit_box_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: unit_box_requirements.size,
                    align_mask: unit_box_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::DEVICE_ADDRESS
                        | gpu_alloc::UsageFlags::TRANSIENT
                        | gpu_alloc::UsageFlags::UPLOAD,
                    memory_types: unit_box_requirements.memory_type_bits,
                },
            )
            .unwrap();
        unit_box_mem
            .write_bytes(
                AshMemoryDevice::wrap(&device),
                0,
                std::slice::from_raw_parts(
                    &unit_box as *const _ as *const u8,
                    std::mem::size_of_val(&unit_box),
                ),
            )
            .unwrap();
        device
            .bind_buffer_memory(
                unit_box_buffer,
                *unit_box_mem.memory(),
                unit_box_mem.offset(),
            )
            .unwrap();
        let unit_box_device_address = device.get_buffer_device_address(
            &vk::BufferDeviceAddressInfo::builder()
                .buffer(unit_box_buffer)
                .build(),
        );
        let geometry = [vk::AccelerationStructureGeometryKHR::builder()
            .geometry_type(vk::GeometryTypeKHR::AABBS)
            .flags(vk::GeometryFlagsKHR::default())
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                aabbs: vk::AccelerationStructureGeometryAabbsDataKHR::builder()
                    .data(vk::DeviceOrHostAddressConstKHR {
                        device_address: unit_box_device_address,
                    })
                    .stride(std::mem::size_of_val(&unit_box) as u64)
                    .build(),
            })
            .build()];
        let mut build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .geometries(&geometry)
            .build();
        let sizes = acceleration_structure_loader.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::DEVICE,
            &build_geometry_info,
            &[1],
        );

        let unit_box_as_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .flags(vk::BufferCreateFlags::default())
                    .size(sizes.acceleration_structure_size)
                    .usage(vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let unit_box_as_buf_requirements = device.get_buffer_memory_requirements(unit_box_as_buf);
        let unit_box_as_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: unit_box_as_buf_requirements.size,
                    align_mask: unit_box_as_buf_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::FAST_DEVICE_ACCESS,
                    memory_types: unit_box_as_buf_requirements.memory_type_bits,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(
                unit_box_as_buf,
                *unit_box_as_mem.memory(),
                unit_box_as_mem.offset(),
            )
            .unwrap();

        let scratch_alignment = device_info
            .acceleration_structure_properties
            .min_acceleration_structure_scratch_offset_alignment
            as u64;
        let scratch_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .flags(vk::BufferCreateFlags::default())
                    .size(sizes.build_scratch_size + scratch_alignment)
                    .usage(
                        vk::BufferUsageFlags::STORAGE_BUFFER
                            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let scratch_requirements = device.get_buffer_memory_requirements(scratch_buf);

        let scratch_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: scratch_requirements.size,
                    align_mask: scratch_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::FAST_DEVICE_ACCESS
                        | gpu_alloc::UsageFlags::TRANSIENT
                        | gpu_alloc::UsageFlags::DEVICE_ADDRESS,
                    memory_types: scratch_requirements.memory_type_bits,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(scratch_buf, *scratch_mem.memory(), scratch_mem.offset())
            .unwrap();
        let scratch_device_address = device.get_buffer_device_address(
            &vk::BufferDeviceAddressInfo::builder()
                .buffer(scratch_buf)
                .build(),
        );
        // Round up
        let scratch_device_address = ((scratch_device_address + scratch_alignment - 1)
            / scratch_alignment)
            * scratch_alignment;
        println!("Creating unit box");
        let unit_box_as = acceleration_structure_loader
            .create_acceleration_structure(
                &vk::AccelerationStructureCreateInfoKHR::builder()
                    .buffer(unit_box_as_buf)
                    .offset(0)
                    .size(sizes.acceleration_structure_size)
                    .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
                    // .device_address()
                    .build(),
                None,
            )
            .unwrap();
        let unit_box_as_device_address = acceleration_structure_loader
            .get_acceleration_structure_device_address(
                &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                    .acceleration_structure(unit_box_as)
                    .build(),
            );
        build_geometry_info.dst_acceleration_structure = unit_box_as;
        build_geometry_info.scratch_data.device_address = scratch_device_address;

        device
            .begin_command_buffer(
                command_buffer,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                    .build(),
            )
            .unwrap();
        acceleration_structure_loader.cmd_build_acceleration_structures(
            command_buffer,
            &[build_geometry_info],
            &[&[vk::AccelerationStructureBuildRangeInfoKHR {
                primitive_count: 1,
                primitive_offset: 0,
                first_vertex: 0,
                transform_offset: 0,
            }]],
        );
        device.end_command_buffer(command_buffer).unwrap();

        let fence = device
            .create_fence(
                &vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::default()),
                None,
            )
            .unwrap();
        device
            .queue_submit(
                queues.compute_queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[command_buffer])
                    .build()],
                fence,
            )
            .unwrap();

        let tlas_state = TlasState {
            tlas: vk::AccelerationStructureKHR::null(),
            tlas_buf: vk::Buffer::null(),
            tlas_mem: None,
            tlas_data_buf: vk::Buffer::null(),
            tlas_data_mem: None,
            tlas_scratch_buf: vk::Buffer::null(),
            tlas_scratch_mem: None,
            unit_box_as,
            unit_box_as_buf,
            unit_box_as_mem,
            unit_box_as_device_address,
            unit_box_scratch_buf: scratch_buf,
            unit_box_scratch_mem: Some(scratch_mem),
            command_pool,
            command_buffer,
            fence,
            have_updates_pending: true,
            needs_update_next_frame: false,
        };
        app.insert_resource(tlas_state);
    }
}

fn tlas_update(
    mut render_world: ResMut<RenderWorld>,
    voxel_models: Res<Assets<crate::VoxelModel>>,
    mut voxel_model_events: EventReader<AssetEvent<crate::VoxelModel>>,
    anything_changed_query: Query<
        (&GlobalTransform, &Raytraced, &Handle<crate::VoxelModel>),
        Or<(
            Changed<GlobalTransform>,
            Changed<Raytraced>,
            Changed<Handle<crate::VoxelModel>>,
        )>,
    >,
    entities_query: Query<(&GlobalTransform, &Raytraced, &Handle<crate::VoxelModel>)>,
) {
    let render_world = &mut *render_world;
    let (
        device,
        mut state,
        queues,
        acceleration_structure_loader,
        mut allocator,
        device_info,
        mut uniform_arr,
    ) = SystemState::<(
        Res<ash::Device>,
        ResMut<TlasState>,
        Res<Queues>,
        Res<ash::extensions::khr::AccelerationStructure>,
        ResMut<crate::Allocator>,
        Res<DeviceInfo>,
        ResMut<uniform::UniformArray>,
    )>::new(render_world)
    .get_mut(render_world);

    let have_updates_this_frame =
        !anything_changed_query.is_empty() || voxel_model_events.iter().next().is_some(); // have updates this frame
    if !state.should_update(&device, &mut allocator, have_updates_this_frame) {
        return;
    }

    let mut models_in_use: Vec<Handle<VoxelModel>> = Vec::new();
    let mut model_to_index: HashMap<Handle<VoxelModel>, u32> = HashMap::default();
    // do updates
    let data: Vec<_> = entities_query
        .iter()
        .filter(|(_, _, model)| !voxel_models.get(*model).is_none())
        .map(|(transform, aabb, model_handle)| {
            let custom_index: u32 =
                if let Some(index) = model_to_index.get(&Handle::weak(model_handle.id)) {
                    *index
                } else {
                    let index = models_in_use.len() as u32;
                    models_in_use.push(Handle::weak(model_handle.id));
                    model_to_index.insert(Handle::weak(model_handle.id), index);
                    index
                };
            assert_eq!(custom_index & !0xFFFFFF, 0, "Index Overflow");
            // We use the same unit box BLAS for all instances. So, we change the shape of the unit box by streching it.
            let scale = transform.scale * aabb.aabb_extent;
            let mat = Mat4::from_scale_rotation_translation(
                scale,
                transform.rotation,
                transform.translation,
            );
            let mat = mat.transpose().to_cols_array();

            let mask: u8 = 0xFF;
            println!("data is {:?}, custom index is {}", aabb, custom_index);
            unsafe {
                let mut instance = vk::AccelerationStructureInstanceKHR {
                    transform: vk::TransformMatrixKHR {
                        matrix: MaybeUninit::uninit().assume_init(),
                    },
                    instance_custom_index_and_mask: ((mask as u32) << 24)
                        | (custom_index & 0xFFFFFF),
                    instance_shader_binding_table_record_offset_and_flags: 0,
                    acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                        device_handle: state.unit_box_as_device_address,
                    },
                };
                instance.transform.matrix.copy_from_slice(&mat[0..12]);
                instance
            }
        })
        .collect();
    if data.len() == 0 {
        unsafe {
            // TODO: figure out a way to delete the AS
            if state.tlas_buf != vk::Buffer::null() {
                device.destroy_buffer(state.tlas_buf, None);
                state.tlas_buf = vk::Buffer::null();
            }
            if let Some(mem) = state.tlas_mem.take() {
                allocator.dealloc(AshMemoryDevice::wrap(&*device), mem);
            }
        }
        debug_assert!(state.tlas_mem.is_none());
        debug_assert_eq!(state.tlas_buf, vk::Buffer::null());
        println!(
            "Deleted TLAS, before {:?}, after {:?}",
            state.tlas,
            vk::AccelerationStructureKHR::null()
        );
        state.tlas = vk::AccelerationStructureKHR::null();
        state.tlas_buf = vk::Buffer::null();
        state.tlas_mem = None;
        return;
    }

    unsafe {
        uniform_arr.write(
            models_in_use.iter().map(|handle| {
                let model = voxel_models.get(handle).unwrap(); // We already made sure that the model was loaded.
                UniformEntry {
                    device: uniform::DeviceAddress(model.svdag.arena.get_buffer_device_address()),
                    parent: model.svdag.get_roots()[0].get_value(),
                }
            }),
            &device,
            &mut allocator,
        );
    }

    println!("models in use are {:?}", models_in_use);
    let data_device_addr = unsafe {
        let data_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .size(std::mem::size_of_val(&data) as u64)
                    .usage(
                        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let data_buf_requirements = device.get_buffer_memory_requirements(data_buf);
        let mut data_buf_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: data_buf_requirements.size,
                    align_mask: data_buf_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::UPLOAD,
                    memory_types: data_buf_requirements.memory_type_bits,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(data_buf, *data_buf_mem.memory(), data_buf_mem.offset())
            .unwrap();
        data_buf_mem
            .write_bytes(
                AshMemoryDevice::wrap(&*device),
                0,
                std::slice::from_raw_parts(
                    data.as_slice() as *const _ as *const u8,
                    std::mem::size_of_val(data.as_slice()),
                ),
            )
            .unwrap();
        debug_assert_eq!(state.tlas_data_buf, vk::Buffer::null());
        debug_assert!(state.tlas_data_mem.is_none());
        state.tlas_data_buf = data_buf;
        state.tlas_data_mem = Some(data_buf_mem);
        device.get_buffer_device_address(
            &vk::BufferDeviceAddressInfo::builder()
                .buffer(data_buf)
                .build(),
        )
    };

    let build_geometry = [vk::AccelerationStructureGeometryKHR::builder()
        .geometry_type(vk::GeometryTypeKHR::INSTANCES)
        .flags(vk::GeometryFlagsKHR::default())
        .geometry(vk::AccelerationStructureGeometryDataKHR {
            instances: vk::AccelerationStructureGeometryInstancesDataKHR::builder()
                .array_of_pointers(false)
                .data(vk::DeviceOrHostAddressConstKHR {
                    device_address: data_device_addr,
                })
                .build(),
        })
        .build()];

    let mut build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
        .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
        .geometries(&build_geometry)
        .build();

    unsafe {
        let sizes = acceleration_structure_loader.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::DEVICE,
            &build_geometry_info,
            &[data.len() as u32],
        );
        let scratch_alignment = device_info
            .acceleration_structure_properties
            .min_acceleration_structure_scratch_offset_alignment
            as u64;
        let scratch_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .flags(vk::BufferCreateFlags::default())
                    .size(sizes.build_scratch_size + scratch_alignment)
                    .usage(
                        vk::BufferUsageFlags::STORAGE_BUFFER
                            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let scratch_requirements = device.get_buffer_memory_requirements(scratch_buf);

        let scratch_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: scratch_requirements.size,
                    align_mask: scratch_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::FAST_DEVICE_ACCESS
                        | gpu_alloc::UsageFlags::TRANSIENT
                        | gpu_alloc::UsageFlags::DEVICE_ADDRESS,
                    memory_types: scratch_requirements.memory_type_bits,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(scratch_buf, *scratch_mem.memory(), scratch_mem.offset())
            .unwrap();
        let scratch_device_address = device.get_buffer_device_address(
            &vk::BufferDeviceAddressInfo::builder()
                .buffer(scratch_buf)
                .build(),
        );
        let scratch_device_address =
            crate::util::round_up(scratch_device_address, scratch_alignment);

        debug_assert_eq!(state.tlas_scratch_buf, vk::Buffer::null());
        debug_assert!(state.tlas_scratch_mem.is_none());
        state.tlas_scratch_buf = scratch_buf;
        state.tlas_scratch_mem = Some(scratch_mem);

        let as_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .flags(vk::BufferCreateFlags::default())
                    .size(sizes.acceleration_structure_size)
                    .usage(
                        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let as_buf_requirements = device.get_buffer_memory_requirements(as_buf);

        let as_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: as_buf_requirements.size,
                    align_mask: as_buf_requirements.alignment,
                    usage: gpu_alloc::UsageFlags::FAST_DEVICE_ACCESS,
                    memory_types: as_buf_requirements.memory_type_bits,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(as_buf, *as_mem.memory(), as_mem.offset())
            .unwrap();
        println!("creating tlas");
        let tlas = acceleration_structure_loader
            .create_acceleration_structure(
                &vk::AccelerationStructureCreateInfoKHR::builder()
                    .buffer(as_buf)
                    .offset(0)
                    .size(sizes.acceleration_structure_size)
                    .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
                    // .device_address()
                    .build(),
                None,
            )
            .unwrap();
        build_geometry_info.dst_acceleration_structure = tlas;
        build_geometry_info.scratch_data.device_address = scratch_device_address;

        debug_assert!(state.command_buffer != vk::CommandBuffer::null());
        device
            .begin_command_buffer(
                state.command_buffer,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                    .build(),
            )
            .unwrap();
        acceleration_structure_loader.cmd_build_acceleration_structures(
            state.command_buffer,
            &[build_geometry_info],
            &[&[vk::AccelerationStructureBuildRangeInfoKHR {
                primitive_count: data.len() as u32,
                primitive_offset: 0,
                first_vertex: 0,
                transform_offset: 0,
            }]],
        );
        device.end_command_buffer(state.command_buffer).unwrap();

        device
            .queue_submit(
                queues.compute_queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[state.command_buffer])
                    .build()],
                state.fence,
            )
            .unwrap();

        println!("We did it");
        /*
        if state.tlas != vk::AccelerationStructureKHR::null() {
            acceleration_structure_loader.destroy_acceleration_structure(state.tlas, None);
            state.tlas = vk::AccelerationStructureKHR::null();
        }
        */
        // TODO: figure out a way to delete the AS
        if state.tlas_buf != vk::Buffer::null() {
            device.destroy_buffer(state.tlas_buf, None);
            state.tlas_buf = vk::Buffer::null();
        }
        if let Some(mem) = state.tlas_mem.take() {
            allocator.dealloc(AshMemoryDevice::wrap(&*device), mem);
        }

        debug_assert!(state.tlas_mem.is_none());
        debug_assert_eq!(state.tlas_buf, vk::Buffer::null());
        println!("Swapping TLAS, before {:?}, after {:?}", state.tlas, tlas);
        state.tlas = tlas;
        state.tlas_buf = as_buf;
        state.tlas_mem = Some(as_mem);
        state.did_updates();
    }
}
