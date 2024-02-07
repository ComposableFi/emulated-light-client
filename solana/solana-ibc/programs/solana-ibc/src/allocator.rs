#[cfg(all(
    target_os = "solana",
    feature = "custom-heap",
    not(feature = "no-entrypoint"),
))]
#[global_allocator]
static ALLOCATOR: solana_allocator::BumpAllocator = {
    // SAFETY: We’re only instantiating the BumpAllocator once.
    unsafe { solana_allocator::BumpAllocator::new() }
};
