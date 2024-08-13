//! Defines custom allocator (when necessary) and wraps access to global state.
//!
//! This module serves two purposes.  First of all, when running on Solana, it
//! defines a custom allocator which can handle heap sizes larger than 32 KiB.
//! Default allocator defined by solana_program assumes heap is 32 KiB.  We’re
//! replacing it with a custom one which can handle heaps of arbitrary size.
//!
//! Second of all, Solana doesn’t allow mutable global variables.  We’re working
//! around that by allocating global state on the heap.  This is done by the
//! custom allocator.  This module than provides a [`global`] function which
//! returns `Global` type with all the available global variables.  While the
//! returned reference is static, the variables may use inner mutability.

#[allow(unexpected_cfgs)]
#[cfg(all(
    target_os = "solana",
    feature = "custom-heap",
    not(feature = "no-entrypoint"),
    not(test),
))]
mod imp {
    #[allow(unused_imports)] // needed for nightly
    use alloc::boxed::Box;
    use core::cell::Cell;

    use sigverify::Verifier;

    /// The global state available to the smart contract.
    #[derive(bytemuck::Zeroable)]
    pub(crate) struct Global {
        verifier: Cell<Option<&'static Verifier<'static>>>,
    }

    impl Global {
        /// Returns global verifier, if initialised.
        pub fn verifier(&self) -> Option<&'static Verifier<'static>> {
            self.verifier.get()
        }

        /// Takes ownership of the verifier and sets it as the global verifier.
        ///
        /// This operation leaks memory thus it shouldn’t be called multiple
        /// times.  It’s intended to be called at most once at the start of the
        /// program.
        pub fn set_verifier(&self, verifier: Verifier<'static>) {
            // Allocate the verifier on heap so it has fixed address, then leak
            // so it has static lifetime.
            self.verifier.set(Some(Box::leak(Box::new(verifier))))
        }
    }

    // SAFETY: Global is in fact not Sync.  However, Solana is single-threaded
    // so we don’t need to worry about thread safety.  Since this implementation
    // is used when building for Solana, we can safely lie to the compiler about
    // Global being Sync.
    //
    // We need Global to be Sync because it’s !Sync status percolates to
    // BumpAllocator<Global> and since that’s a static variable, Rust requires
    // that it’s Sync.
    unsafe impl core::marker::Sync for Global {}

    #[global_allocator]
    static ALLOCATOR: solana_allocator::BumpAllocator<Global> = {
        // SAFETY: We’re only instantiating the BumpAllocator once and setting
        // it as global allocator.
        unsafe { solana_allocator::BumpAllocator::new() }
    };

    /// Returns reference to the global state.
    pub(crate) fn global() -> &'static Global { ALLOCATOR.global() }
}

#[allow(unexpected_cfgs)]
#[cfg(any(
    not(target_os = "solana"),
    not(feature = "custom-heap"),
    feature = "no-entrypoint",
    test,
))]
mod imp {
    use sigverify::Verifier;

    /// The global state available to the smart contract.
    ///
    /// Note that we don’t support the global state in tests or CPI.  None of
    /// the unit tests will use code which relies on global state.  Similarly,
    /// we don’t expose any types of functions which use global state so crates
    /// which depend on us for CPI won’t need the global state.
    pub(crate) enum Global {}

    impl Global {
        /// Returns global verifier, if initialised.
        pub fn verifier(&self) -> Option<&'static Verifier<'static>> {
            match *self {}
        }

        /// Takes ownership of the verifier and sets it as the global verifier.
        ///
        /// This operation leaks memory thus it shouldn’t be called multiple
        /// times.  It’s intended to be called at most once at the start of the
        /// program.
        pub fn set_verifier(&self, _verifier: Verifier<'static>) {
            match *self {}
        }
    }

    pub(crate) fn global() -> &'static Global {
        unimplemented!("global should never be called in tests or CPI")
    }
}

pub(crate) use imp::global;
