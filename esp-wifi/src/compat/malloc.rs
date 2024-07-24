use core::alloc::Layout;

pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    trace!("alloc {}", size);

    let total_size = size + 4;

    let layout = Layout::from_size_align_unchecked(total_size, 4);
    let ptr = alloc::alloc::alloc(layout);

    if ptr.is_null() {
        warn!("Unable to allocate {} bytes", size);
        return ptr;
    }

    *(ptr as *mut usize) = total_size;
    ptr.offset(4)
}

pub unsafe extern "C" fn free(ptr: *mut u8) {
    trace!("free {:?}", ptr);

    if ptr.is_null() {
        return;
    }

    let ptr = ptr.offset(-4);
    let total_size = *(ptr as *const usize);

    let layout = Layout::from_size_align_unchecked(total_size, 4);
    alloc::alloc::dealloc(ptr, layout);
}

#[no_mangle]
pub unsafe extern "C" fn calloc(number: u32, size: usize) -> *mut u8 {
    trace!("calloc {} {}", number, size);

    let total_size = number as usize * size;
    let ptr = malloc(total_size);

    if !ptr.is_null() {
        for i in 0..total_size as isize {
            ptr.offset(i).write_volatile(0);
        }
    }

    ptr
}
