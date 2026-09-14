//! Embedded previews inside TIFF-based RAW containers.
//!
//! §7 of the architecture describes a two-stage render: the embedded preview at
//! selection, the baseline development once the selection settles. This module
//! is the first stage and nothing more. It never touches the sensor mosaic, so
//! what it returns is what the camera already developed, with its own tone
//! curve and sharpening. ADR 0004 forbids substituting that for RAW data; the
//! rule it states is against doing so silently, so every caller here is
//! required to say which stage produced the pixels.
//!
//! The input is untrusted and the parser is deliberately small: fixed limits on
//! how many directories and entries are read, every offset checked against the
//! buffer, one level of sub-directories, and no allocation proportional to
//! anything the file declares.
use anyhow::{Result, bail, ensure};

/// Where an embedded JPEG lives, and how the container says to orient it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preview {
    pub offset: usize,
    pub length: usize,
    /// Selected IFD orientation, falling back to the primary IFD only.
    /// None preserves the JPEG's own orientation; Some(1) is explicit.
    pub orientation: Option<u16>,
}

/// Directories and entries a well-formed container needs. Anything past these
/// is a malformed or hostile file, not a photograph.
const MAX_DIRECTORIES: usize = 8;
const MAX_ENTRIES: usize = 512;
const MAX_SUBDIRECTORIES: usize = 16;
/// An embedded preview is a convenience copy, never a gigapixel payload.
const MAX_PREVIEW_BYTES: usize = 64 * 1024 * 1024;

struct Reader<'a> {
    bytes: &'a [u8],
    big_endian: bool,
}

impl<'a> Reader<'a> {
    fn short(&self, at: usize) -> Result<u16> {
        let slice: [u8; 2] = self
            .bytes
            .get(at..at + 2)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| anyhow::anyhow!("Contenitore troncato"))?;
        Ok(if self.big_endian {
            u16::from_be_bytes(slice)
        } else {
            u16::from_le_bytes(slice)
        })
    }
    fn long(&self, at: usize) -> Result<u32> {
        let slice: [u8; 4] = self
            .bytes
            .get(at..at + 4)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| anyhow::anyhow!("Contenitore troncato"))?;
        Ok(if self.big_endian {
            u32::from_be_bytes(slice)
        } else {
            u32::from_le_bytes(slice)
        })
    }
    /// Value of an entry whose count is one, whatever integer width it uses.
    fn scalar(&self, value_at: usize, kind: u16) -> Result<u32> {
        match kind {
            3 => Ok(u32::from(self.short(value_at)?)),
            4 => self.long(value_at),
            _ => bail!("Tipo di campo non atteso"),
        }
    }
}

/// True when the bytes open like a TIFF container. RAW files from Nikon, Canon,
/// Sony and Adobe all do; ordinary TIFF images can also carry previews.
pub fn is_tiff_container(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4), Some(b"II\x2a\x00") | Some(b"MM\x00\x2a"))
}

/// The largest embedded JPEG in the container.
///
/// Largest by stored length, which tracks resolution closely enough to choose
/// between a full-size preview and a thumbnail without decoding either. The
/// bytes are confirmed to start with an actual JPEG marker before being
/// returned, so a wrong offset fails here rather than inside a decoder.
pub fn embedded_preview(bytes: &[u8]) -> Result<Preview> {
    let (_, preview) = inspect(bytes)?;
    preview.ok_or_else(|| anyhow::anyhow!("Nessuna anteprima incorporata"))
}

/// Positive RAW declarations, independent of the presence of a JPEG thumbnail.
/// Unknown or malformed TIFF metadata never authorizes a preview substitution.
pub fn declares_raw(bytes: &[u8]) -> bool {
    inspect(bytes).is_ok_and(|(raw, _)| raw)
}

fn inspect(bytes: &[u8]) -> Result<(bool, Option<Preview>)> {
    ensure!(is_tiff_container(bytes), "Non è un contenitore TIFF");
    let reader = Reader {
        bytes,
        big_endian: bytes[0] == b'M',
    };
    let primary = reader.long(4)? as usize;
    let mut directories = vec![primary];
    let mut primary_orientation = None;
    let mut best: Option<Preview> = None;
    let mut raw = false;
    let mut visited = 0usize;

    while let Some(at) = directories.pop() {
        visited += 1;
        if visited > MAX_DIRECTORIES || at == 0 {
            break;
        }
        let count = reader.short(at)? as usize;
        ensure!(
            count <= MAX_ENTRIES,
            "Directory del contenitore fuori quota"
        );
        let mut offset = None;
        let mut length = None;
        let mut orientation = None;
        for index in 0..count {
            let entry = at + 2 + index * 12;
            let tag = reader.short(entry)?;
            let kind = reader.short(entry + 2)?;
            let items = reader.long(entry + 4)?;
            let value_at = entry + 8;
            match tag {
                // CFA/LinearRaw photometric interpretation or DNGVersion.
                0x0106 if items == 1 => {
                    raw |= matches!(reader.scalar(value_at, kind)?, 32803 | 34892);
                }
                0xc612 if kind == 1 && items == 4 => raw = true,
                // Orientation belongs to this IFD. Explicit 1 is a value,
                // never an invitation for a later thumbnail to overwrite it.
                0x0112 if items == 1 && orientation.is_none() => {
                    let stated = reader.scalar(value_at, kind).unwrap_or(0);
                    if (1..=8).contains(&stated) {
                        orientation = Some(stated as u16);
                    }
                }
                // SubIFDs: where every camera hides its full-size preview.
                0x014a => {
                    let items = items as usize;
                    ensure!(items <= MAX_SUBDIRECTORIES, "Troppe sotto-directory");
                    // Four bytes or fewer live in the entry; more are pointed to.
                    let base = if items <= 1 {
                        value_at
                    } else {
                        reader.long(value_at)? as usize
                    };
                    for i in 0..items {
                        directories.push(reader.long(base + i * 4)? as usize);
                    }
                }
                0x0201 if items == 1 => offset = Some(reader.long(value_at)? as usize),
                0x0202 if items == 1 => length = Some(reader.long(value_at)? as usize),
                _ => {}
            }
        }
        if at == primary {
            primary_orientation = orientation;
        }
        // The chain of top-level directories; a thumbnail often lives in the second.
        let next = reader.long(at + 2 + count * 12)? as usize;
        if next != 0 {
            directories.push(next);
        }
        if let (Some(offset), Some(length)) = (offset, length) {
            let inside = (2..=MAX_PREVIEW_BYTES).contains(&length)
                && offset.checked_add(length).is_some_and(|e| e <= bytes.len());
            // A JPEG and nothing else: a stale offset stops here, not later.
            if inside
                && bytes.get(offset..offset + 2) == Some(&[0xFF, 0xD8])
                && best.is_none_or(|previous| length > previous.length)
            {
                best = Some(Preview {
                    offset,
                    length,
                    orientation: orientation.or(primary_orientation),
                });
            }
        }
    }

    Ok((raw, best))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Little-endian container: header, one directory, a JPEG payload.
    fn container(orientation: u16, payload: &[u8]) -> Vec<u8> {
        let mut bytes = b"II\x2a\x00".to_vec();
        bytes.extend_from_slice(&8u32.to_le_bytes());
        let entries: u16 = 3;
        let payload_at = 8 + 2 + entries as usize * 12 + 4;
        bytes.extend_from_slice(&entries.to_le_bytes());
        for (tag, kind, value) in [
            (0x0112u16, 3u16, u32::from(orientation)),
            (0x0201, 4, payload_at as u32),
            (0x0202, 4, payload.len() as u32),
        ] {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn a_declared_preview_is_found_with_its_orientation() {
        let payload = [0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x01];
        let bytes = container(6, &payload);
        let preview = embedded_preview(&bytes).unwrap();
        assert_eq!(preview.length, payload.len());
        assert_eq!(preview.orientation, Some(6));
        assert_eq!(&bytes[preview.offset..][..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn selected_ifd_owns_orientation_with_primary_only_as_fallback() {
        for (local, expected) in [(Some(1u16), Some(1)), (Some(3), Some(3)), (None, Some(6))] {
            let mut bytes = container(6, &[0xff, 0xd8, 0]);
            let at = bytes.len() as u32;
            bytes[46..50].copy_from_slice(&at.to_le_bytes());
            let offset = at + 2 + 3 * 12 + 4;
            bytes.extend(3u16.to_le_bytes());
            for (tag, kind, value) in [
                (
                    if local.is_some() { 274u16 } else { 65000 },
                    3u16,
                    u32::from(local.unwrap_or(0)),
                ),
                (513, 4, offset),
                (514, 4, 5),
            ] {
                bytes.extend(tag.to_le_bytes());
                bytes.extend(kind.to_le_bytes());
                bytes.extend(1u32.to_le_bytes());
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(0u32.to_le_bytes());
            bytes.extend([0xff, 0xd8, 0, 1, 2]);
            let selected = embedded_preview(&bytes).unwrap();
            assert_eq!(selected.offset, offset as usize);
            assert_eq!(selected.orientation, expected);
        }
    }

    /// An offset that leaves the file, a length that overruns it and bytes that
    /// are not a JPEG must all fail here rather than reach a decoder.
    #[test]
    fn a_preview_outside_the_file_or_not_a_jpeg_is_refused() {
        let mut truncated = container(1, &[0xFF, 0xD8, 0xFF]);
        truncated.truncate(truncated.len() - 2);
        assert!(embedded_preview(&truncated).is_err());
        assert!(embedded_preview(&container(1, &[0x00, 0x00, 0x00, 0x00])).is_err());
        assert!(embedded_preview(b"not a container at all").is_err());
    }

    /// A PNG is not a container, so the caller keeps its ordinary path.
    #[test]
    fn only_tiff_headers_are_treated_as_containers() {
        assert!(!is_tiff_container(b"\x89PNG\r\n\x1a\n"));
        assert!(is_tiff_container(b"II\x2a\x00rest"));
        assert!(is_tiff_container(b"MM\x00\x2arest"));
    }

    #[test]
    fn preview_signature_at_eof_is_checked_for_every_short_length() {
        for payload in [&[][..], &[0xff][..], &[0xff, 0xd8][..]] {
            let bytes = container(1, payload);
            assert_eq!(embedded_preview(&bytes).is_ok(), payload.len() == 2);
        }
    }
}
