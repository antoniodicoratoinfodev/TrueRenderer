//! Read the kernel's WIN://NOALLAPPPKG UInt64 security attribute.
//! TokenIsLessPrivilegedAppContainer is not implemented on every Windows build.
//! ABI: TOKEN_SECURITY_ATTRIBUTES_INFORMATION / TOKEN_SECURITY_ATTRIBUTE_V1;
//! the token variant has a counted UNICODE_STRING (not the CLAIM variant).
use windows_sys::{
    Wdk::Storage::FileSystem::NtQueryInformationToken,
    Win32::{
        Foundation::{HANDLE, UNICODE_STRING},
        Security::TokenSecurityAttributes,
    },
};

#[repr(C)]
#[derive(Clone, Copy)]
struct Attributes {
    version: u16,
    reserved: u16,
    count: u32,
    attributes: *const Attribute,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Attribute {
    name: UNICODE_STRING,
    kind: u16,
    reserved: u16,
    flags: u32,
    count: u32,
    values: *const u64,
}

pub fn verified(token: HANDLE) -> bool {
    let mut length = 0;
    unsafe {
        NtQueryInformationToken(
            token,
            TokenSecurityAttributes,
            std::ptr::null_mut(),
            0,
            &mut length,
        )
    };
    if !(size_of::<Attributes>() as u32..=65536).contains(&length) {
        return false;
    }
    let mut storage = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    let status = unsafe {
        NtQueryInformationToken(
            token,
            TokenSecurityAttributes,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    };
    if status < 0 || length as usize > size_of_val(storage.as_slice()) {
        return false;
    }
    let start = storage.as_ptr() as usize;
    let end = start + length as usize;
    fn read<T: Copy>(address: usize, start: usize, end: usize) -> Option<T> {
        if address < start || address.checked_add(size_of::<T>())? > end {
            return None;
        }
        // The pointer is within the live, OS-filled allocation; unaligned reads
        // also avoid relying on alignment of individual token attributes.
        Some(unsafe { std::ptr::read_unaligned(address as *const T) })
    }
    let Some(info) = read::<Attributes>(start, start, end) else {
        return false;
    };
    if info.version != 1 || info.count > 256 {
        return false;
    }
    let expected: Vec<u16> = "WIN://NOALLAPPPKG".encode_utf16().collect();
    for index in 0..info.count as usize {
        let Some(address) = (info.attributes as usize).checked_add(index * size_of::<Attribute>())
        else {
            return false;
        };
        let Some(attribute) = read::<Attribute>(address, start, end) else {
            return false;
        };
        if attribute.kind != 2
            || attribute.count != 1
            || attribute.flags & 16 != 0
            || usize::from(attribute.name.Length) != expected.len() * 2
        {
            continue;
        }
        let name = attribute.name.Buffer as usize;
        let matches = expected.iter().enumerate().all(|(i, ch)| {
            name.checked_add(i * 2)
                .and_then(|p| read::<u16>(p, start, end))
                == Some(*ch)
        });
        if matches {
            return read::<u64>(attribute.values as usize, start, end) == Some(1);
        }
    }
    false
}
