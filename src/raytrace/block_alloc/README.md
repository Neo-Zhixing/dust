The block allocator allocates blocks of a fixed size from a larger region of memory requested from vulkan.
It also allows you to flush a specific memory range inside an allocation in batches.
The block allocator supports internal mutability and is safe to allocate / deallocate from multiple threads.
