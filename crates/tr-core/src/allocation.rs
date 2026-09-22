//! Large Rust buffers bypass malloc's deferred large-block reclamation on macOS.
//! Mapping is anonymous/private, never a file or a shared worker allocation.
//! Native codecs keep their own allocator; this is not a physical-memory cap.
use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr;

const MAPPED_MINIMUM: usize = 4 * 1024 * 1024;
// Both supported macOS architectures provide at least 4 KiB page alignment.
// Larger alignments stay with System, including allocation/deallocation pairs.
fn mapped(layout: Layout) -> bool {
    layout.size() >= MAPPED_MINIMUM && layout.align() <= 4096
}

struct LargeBufferAllocator;
#[global_allocator]
static ALLOCATOR: LargeBufferAllocator = LargeBufferAllocator;

// SAFETY: backend selection depends only on the supplied Layout, which callers
// must preserve for deallocation. mmap returns disjoint page-aligned zeroed
// storage; munmap receives that exact base and size. No locks, allocations,
// formatting or unwinding occur inside allocator methods.
unsafe impl GlobalAlloc for LargeBufferAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !mapped(layout) {
            // SAFETY: caller supplies a nonzero valid Layout.
            return unsafe { System.alloc(layout) };
        }
        // SAFETY: anonymous mapping, no requested address, descriptor or offset.
        let memory = unsafe {
            libc::mmap(
                ptr::null_mut(),
                layout.size(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if memory == libc::MAP_FAILED {
            ptr::null_mut()
        } else {
            memory.cast()
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if mapped(layout) {
            // SAFETY: fresh anonymous pages are zero-filled by the kernel.
            unsafe { self.alloc(layout) }
        } else {
            // SAFETY: caller supplies a valid Layout.
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn dealloc(&self, memory: *mut u8, layout: Layout) {
        if mapped(layout) {
            // SAFETY: same base and Layout used by alloc; munmap rounds the
            // length to the same page boundary as mmap. No aliases remain.
            unsafe { libc::munmap(memory.cast(), layout.size()) };
        } else {
            // SAFETY: this Layout selected System at allocation time too.
            unsafe { System.dealloc(memory, layout) };
        }
    }

    unsafe fn realloc(&self, memory: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(next) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        if !mapped(layout) && !mapped(next) {
            // SAFETY: both layouts use System; caller owns memory and new_size > 0.
            return unsafe { System.realloc(memory, layout, new_size) };
        }
        // SAFETY: valid nonzero next Layout. On failure the original stays live.
        let replacement = unsafe { self.alloc(next) };
        if !replacement.is_null() {
            // SAFETY: disjoint live allocations with at least the copied size.
            unsafe {
                ptr::copy_nonoverlapping(memory, replacement, layout.size().min(new_size));
                self.dealloc(memory, layout);
            }
        }
        replacement
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_alignment_growth_and_shrink_cross_allocator_boundaries() {
        for alignment in [16, 4096, 8192] {
            let mut layout = Layout::from_size_align(1024, alignment).unwrap();
            // SAFETY: every successful allocation is checked and freed with its
            // current Layout; only initialized bytes are read after resizing.
            unsafe {
                let mut memory = ALLOCATOR.alloc_zeroed(layout);
                assert!(!memory.is_null());
                assert_eq!(memory as usize % alignment, 0);
                assert!(
                    std::slice::from_raw_parts(memory, layout.size())
                        .iter()
                        .all(|b| *b == 0)
                );
                ptr::write_bytes(memory, 0xa5, layout.size());
                for size in [MAPPED_MINIMUM + 17, MAPPED_MINIMUM * 2 + 33, 2048] {
                    let next = ALLOCATOR.realloc(memory, layout, size);
                    assert!(!next.is_null());
                    assert_eq!(next as usize % alignment, 0);
                    assert!(
                        std::slice::from_raw_parts(next, layout.size().min(size))
                            .iter()
                            .all(|b| *b == 0xa5)
                    );
                    ptr::write_bytes(next, 0xa5, size);
                    memory = next;
                    layout = Layout::from_size_align(size, alignment).unwrap();
                }
                ALLOCATOR.dealloc(memory, layout);
                let large = Layout::from_size_align(MAPPED_MINIMUM + 1, alignment).unwrap();
                let zeroed = ALLOCATOR.alloc_zeroed(large);
                assert!(!zeroed.is_null());
                assert!(
                    std::slice::from_raw_parts(zeroed, large.size())
                        .iter()
                        .all(|b| *b == 0)
                );
                ALLOCATOR.dealloc(zeroed, large);
            }
        }
    }

    #[test]
    fn failed_mapping_preserves_the_old_allocation() {
        let layout = Layout::from_size_align(1024, 16).unwrap();
        // Larger than the macOS user address space: mmap must fail, not return
        // a truncated mapping or consume the still-owned original buffer.
        unsafe {
            let memory = ALLOCATOR.alloc(layout);
            assert!(!memory.is_null());
            ptr::write_bytes(memory, 0x5a, layout.size());
            let impossible = isize::MAX as usize - 4096;
            assert!(ALLOCATOR.realloc(memory, layout, impossible).is_null());
            assert!(
                std::slice::from_raw_parts(memory, layout.size())
                    .iter()
                    .all(|b| *b == 0x5a)
            );
            ALLOCATOR.dealloc(memory, layout);
        }
    }
}
