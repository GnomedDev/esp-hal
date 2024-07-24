//! A simple `no_std` heap allocator for RISC-V and Xtensa processors from
//! Espressif. Supports all currently available ESP32 devices.
//!
//! **NOTE:** using this as your global allocator requires using Rust 1.68 or
//! greater, or the `nightly` release channel.
//!
//! # Using this as your Global Allocator
//! To use EspHeap as your global allocator, you need at least Rust 1.68 or
//! nightly.
//!
//! ```rust
//! #[global_allocator]
//! static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();
//!
//! fn init_heap() {
//!     const HEAP_SIZE: usize = 32 * 1024;
//!     static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();
//!
//!     unsafe {
//!         ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
//!     }
//! }
//! ```
//!
//! # Using this with the nightly `allocator_api`-feature
//! Sometimes you want to have single allocations in PSRAM, instead of an esp's
//! DRAM. For that, it's convenient to use the nightly `allocator_api`-feature,
//! which allows you to specify an allocator for single allocations.
//!
//! **NOTE:** To use this, you have to enable the create's `nightly` feature
//! flag.
//!
//! Create and initialize an allocator to use in single allocations:
//! ```rust
//! static PSRAM_ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();
//!
//! fn init_psram_heap() {
//!     unsafe {
//!         PSRAM_ALLOCATOR.init(psram::psram_vaddr_start() as *mut u8, psram::PSRAM_BYTES);
//!     }
//! }
//! ```
//!
//! And then use it in an allocation:
//! ```rust
//! let large_buffer: Vec<u8, _> = Vec::with_capacity_in(1048576, &PSRAM_ALLOCATOR);
//! ```

#![no_std]
#![cfg_attr(feature = "nightly", feature(allocator_api))]
#![doc(html_logo_url = "https://avatars.githubusercontent.com/u/46717278")]

pub mod macros;

#[cfg(feature = "nightly")]
use core::alloc::{AllocError, Allocator};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::RefCell,
    ptr::{self, NonNull},
};

use critical_section::Mutex;
use linked_list_allocator::Heap;

struct EspHeapInner {
    heap: Heap,
    is_global: bool,
}

pub struct EspHeap(Mutex<RefCell<EspHeapInner>>);

impl EspHeap {
    /// Crate a new UNINITIALIZED heap allocator
    ///
    /// You must initialize this heap using the
    /// [`init`](struct.EspHeap.html#method.init) method before using the
    /// allocator.
    pub const fn empty() -> EspHeap {
        EspHeap(Mutex::new(RefCell::new(EspHeapInner {
            heap: Heap::empty(),
            is_global: false,
        })))
    }

    /// Initializes the heap
    ///
    /// This function must be called BEFORE you run any code that makes use of
    /// the allocator.
    ///
    /// `heap_bottom` is a pointer to the location of the bottom of the heap.
    ///
    /// `size` is the size of the heap in bytes.
    ///
    /// Note that:
    ///
    /// - The heap grows "upwards", towards larger addresses. Thus `end_addr`
    ///   must be larger than `start_addr`
    ///
    /// - The size of the heap is `(end_addr as usize) - (start_addr as usize)`.
    ///   The allocator won't use the byte at `end_addr`.
    ///
    /// # Safety
    ///
    /// - The supplied memory region must be available for the entire program (a
    ///   `'static` lifetime).
    /// - The supplied memory region must be exclusively available to the heap
    ///   only, no aliasing.
    /// - This function must be called exactly ONCE.
    /// - `size > 0`.
    pub unsafe fn init(&self, heap_bottom: *mut u8, size: usize) {
        self.init_inner(heap_bottom, size, false);
    }

    /// Initializes the heap as global.
    ///
    /// See [`Self::init`] for the general documentation.
    ///
    /// # Safety
    /// - All safety documentation of [`Self::init`] is met.
    /// - This `EspHeap` is set as the [`global_allocator`].
    pub unsafe fn init_global(&self, heap_bottom: *mut u8, size: usize) {
        self.init_inner(heap_bottom, size, true);
    }

    unsafe fn init_inner(&self, heap_bottom: *mut u8, size: usize, is_global: bool) {
        critical_section::with(|cs| {
            let mut inner = self.0.borrow_ref_mut(cs);
            unsafe { inner.heap.init(heap_bottom, size) };
            inner.is_global = is_global;
        })
    }

    /// Returns if this EspHeap was initialised with [`Self::init_global`].
    ///
    /// This means that all allocation and deallocation requests are guaranteed to be made via the standard `alloc` library will be made via this `EspHeap`.
    pub fn is_global(&self) -> bool {
        critical_section::with(|cs| self.0.borrow_ref_mut(cs).is_global)
    }

    /// Returns an estimate of the amount of bytes in use.
    pub fn used(&self) -> usize {
        critical_section::with(|cs| self.0.borrow_ref_mut(cs).heap.used())
    }

    /// Returns an estimate of the amount of bytes available.
    pub fn free(&self) -> usize {
        critical_section::with(|cs| self.0.borrow_ref_mut(cs).heap.free())
    }
}

unsafe impl GlobalAlloc for EspHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        critical_section::with(|cs| {
            self.0
                .borrow_ref_mut(cs)
                .heap
                .allocate_first_fit(layout)
                .ok()
                .map_or(ptr::null_mut(), |allocation| allocation.as_ptr())
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        critical_section::with(|cs| {
            self.0
                .borrow_ref_mut(cs)
                .heap
                .deallocate(NonNull::new_unchecked(ptr), layout)
        });
    }
}

#[cfg(feature = "nightly")]
unsafe impl Allocator for EspHeap {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        critical_section::with(|cs| {
            let raw_ptr = self
                .heap
                .borrow(cs)
                .borrow_mut()
                .allocate_first_fit(layout)
                .map_err(|_| AllocError)?
                .as_ptr();
            let ptr = NonNull::new(raw_ptr).ok_or(AllocError)?;
            Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
        })
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        self.dealloc(ptr.as_ptr(), layout);
    }
}
