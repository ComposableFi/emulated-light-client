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

use alloc::boxed::Box;
use core::sync::atomic::{AtomicPtr, Ordering};

use solana_ed25519::Verifier;

/// Mutable global state.
#[derive(bytemuck::Zeroable)]
pub(crate) struct Global {
    verifier_ptr: AtomicPtr<Verifier>,
}

impl Global {
    /// Returns global verifier, if initialised.
    pub fn verifier(&self) -> Option<&'static Verifier> {
        let ptr = self.verifier_ptr.load(Ordering::SeqCst);
        // SAFETY: We’ve initialised the pointer from a leaked 'static
        // reference in set_verifier.  It’s thus safe to dereference it.
        unsafe { ptr.as_ref() }
    }

    /// Takes ownership of the verifier and sets it as the global verifier.
    ///
    /// This operation leaks memory thus it shouldn’t be called multiple times.
    /// It’s intended to be called at most once at the start of the program.
    pub fn set_verifier(&self, verifier: Verifier) {
        // Allocate the verifier on heap so it has fixed address and leak so it
        // has 'static lifetime.
        let verifier = Box::leak(Box::new(verifier));
        self.verifier_ptr.store(verifier, Ordering::SeqCst);
    }
}

pub(crate) use imp::global;

#[cfg(all(
    target_os = "solana",
    feature = "custom-heap",
    not(feature = "no-entrypoint"),
    not(test),
))]
mod imp {
    #[global_allocator]
    static ALLOCATOR: solana_allocator::BumpAllocator<super::Global> = {
        // SAFETY: We’re only instantiating the BumpAllocator once and setting
        // it as global allocator.
        unsafe { solana_allocator::BumpAllocator::new() }
    };

    pub(crate) fn global() -> &'static super::Global { ALLOCATOR.global() }
}

#[cfg(any(
    not(target_os = "solana"),
    not(feature = "custom-heap"),
    feature = "no-entrypoint",
    test,
))]
mod imp {
    static GLOBAL: super::Global = super::Global {
        verifier_ptr: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()),
    };

    pub(crate) fn global() -> &'static super::Global { &GLOBAL }
}

/// Returns global verifier if one has been set.
///
/// Together with [`Global::set_verifier`] this function provides an interface
/// analogous to a mutable global.
///
/// Returns `*const Verifier` pointer cast to `*const ()`.  Caller should cast
/// the result back to `*const Verifier` (or better yet `*mut Verifier` and then
/// use `NonNull`).  The pointer conversion is used to avoid [`Verifier`] having
/// to be FFI-safe.
///
/// Due to symbol resolution and cyclical crate dependency shenanigans, this is
/// defined as C function so that it can be accessed from other crates.
/// Client of this interface should declare an extern function and use that to
/// get access to Verifier.
///
/// # Safety
///
/// The function is always safe to run.  If it returns non-null pointer, the
/// pointer is safe to convert to `*const Verifier` and dereferenced.
#[no_mangle]
#[allow(dead_code)]
pub extern "C" fn get_global_ed25519_verifier() -> *const () {
    match global().verifier() {
        None => core::ptr::null(),
        Some(verifier) => verifier as *const Verifier as *const (),
    }
}
