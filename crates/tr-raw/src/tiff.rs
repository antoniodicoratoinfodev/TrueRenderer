//! Bounded classic-TIFF reader over granted bytes. No file I/O or native code.
use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Field<'a> {
    pub kind: u16,
    pub count: usize,
    pub data: &'a [u8],
    le: bool,
}
impl Field<'_> {
    pub fn numbers(self, limit: usize) -> Result<Vec<f64>> {
        ensure!(self.count <= limit, "TIFF: troppi valori nel campo");
        let width = type_width(self.kind)?;
        self.data
            .chunks_exact(width)
            .map(|v| {
                let u16v = |v: &[u8]| {
                    if self.le {
                        u16::from_le_bytes(v.try_into().unwrap())
                    } else {
                        u16::from_be_bytes(v.try_into().unwrap())
                    }
                };
                let u32v = |v: &[u8]| {
                    if self.le {
                        u32::from_le_bytes(v.try_into().unwrap())
                    } else {
                        u32::from_be_bytes(v.try_into().unwrap())
                    }
                };
                let value = match self.kind {
                    1 | 7 => f64::from(v[0]),
                    6 => f64::from(v[0] as i8),
                    3 => f64::from(u16v(v)),
                    8 => f64::from(u16v(v) as i16),
                    4 | 13 => f64::from(u32v(v)),
                    9 => f64::from(u32v(v) as i32),
                    5 | 10 => {
                        let (a, b) = (u32v(&v[..4]), u32v(&v[4..]));
                        let (a, b) = if self.kind == 10 {
                            (f64::from(a as i32), f64::from(b as i32))
                        } else {
                            (f64::from(a), f64::from(b))
                        };
                        ensure!(b != 0., "TIFF: denominatore zero");
                        a / b
                    }
                    11 => f64::from(f32::from_bits(u32v(v))),
                    12 => {
                        let bits = if self.le {
                            u64::from_le_bytes(v.try_into().unwrap())
                        } else {
                            u64::from_be_bytes(v.try_into().unwrap())
                        };
                        f64::from_bits(bits)
                    }
                    _ => bail!("TIFF: campo numerico di tipo non valido"),
                };
                ensure!(value.is_finite(), "TIFF: valore non finito");
                Ok(value)
            })
            .collect()
    }
    pub fn integers(self, limit: usize) -> Result<Vec<u32>> {
        ensure!(
            matches!(self.kind, 1 | 3 | 4 | 13),
            "TIFF: campo intero di tipo non valido"
        );
        self.numbers(limit)?
            .into_iter()
            .map(|v| {
                ensure!(
                    (0. ..=f64::from(u32::MAX)).contains(&v) && v.fract() == 0.,
                    "TIFF: intero fuori scala"
                );
                Ok(v as u32)
            })
            .collect()
    }
    pub fn scalar(self) -> Result<f64> {
        ensure!(self.count == 1, "TIFF: atteso valore singolo");
        Ok(self.numbers(1)?[0])
    }
    pub fn text(self) -> Result<String> {
        ensure!(
            self.kind == 2 && self.count <= 512,
            "TIFF: testo non valido"
        );
        let end = self
            .data
            .iter()
            .position(|b| *b == 0)
            .context("TIFF: testo non terminato")?;
        Ok(String::from_utf8_lossy(&self.data[..end])
            .chars()
            .filter(|c| !c.is_control())
            .collect())
    }
    pub fn signature(self) -> Result<String> {
        ensure!(
            matches!(self.kind, 1 | 2) && self.count > 0 && self.count <= 512,
            "DNG: firma di calibrazione invalida"
        );
        ensure!(self.data.last() == Some(&0), "DNG: firma non terminata");
        let value = &self.data[..self.data.len() - 1];
        ensure!(!value.contains(&0), "DNG: firma con terminazione interna");
        // Signatures are exact UTF-8 identifiers, not sanitised display labels.
        Ok(std::str::from_utf8(value)?.to_owned())
    }
}

#[derive(Debug)]
pub(crate) struct Ifd<'a> {
    pub fields: BTreeMap<u16, Field<'a>>,
}
impl<'a> Ifd<'a> {
    pub fn get(&self, tag: u16) -> Option<Field<'a>> {
        self.fields.get(&tag).copied()
    }
    pub fn required(&self, tag: u16) -> Result<Field<'a>> {
        self.get(tag)
            .with_context(|| format!("TIFF: tag {tag} assente"))
    }
    pub fn integer(&self, tag: u16, default: u32) -> Result<u32> {
        match self.get(tag) {
            Some(f) => {
                ensure!(f.count == 1, "TIFF: atteso intero singolo");
                Ok(f.integers(1)?[0])
            }
            None => Ok(default),
        }
    }
    pub fn text(&self, tag: u16) -> Result<Option<String>> {
        self.get(tag).map(Field::text).transpose()
    }
}

fn type_width(kind: u16) -> Result<usize> {
    Ok(match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => bail!("TIFF: tipo {kind} non supportato"),
    })
}

pub(crate) struct Tiff<'a> {
    pub bytes: &'a [u8],
    pub le: bool,
    pub ifds: Vec<Ifd<'a>>,
}
impl<'a> Tiff<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= crate::MAX_SOURCE_BYTES,
            "RAW: sorgente oltre quota"
        );
        let le = match bytes.get(..4) {
            Some(b"II\x2a\x00") => true,
            Some(b"MM\x00\x2a") => false,
            _ => bail!("RAW: richiesto TIFF classico"),
        };
        let mut reader = Self {
            bytes,
            le,
            ifds: Vec::new(),
        };
        let mut pending = vec![reader.long(4)? as usize];
        let mut seen = HashSet::new();
        let mut total_entries = 0usize;
        let mut metadata_bytes = 0usize;
        while let Some(at) = pending.pop() {
            if at == 0 {
                continue;
            }
            ensure!(seen.insert(at), "TIFF: IFD ciclico o duplicato");
            ensure!(seen.len() <= 32, "TIFF: troppe directory");
            let count = reader.short(at)? as usize;
            total_entries += count;
            ensure!(count <= 1024 && total_entries <= 4096, "TIFF: troppi tag");
            reader.slice(at, 2 + count * 12 + 4)?;
            let mut fields = BTreeMap::new();
            for index in 0..count {
                let entry = at + 2 + 12 * index;
                let tag = reader.short(entry)?;
                let kind = reader.short(entry + 2)?;
                let count = reader.long(entry + 4)? as usize;
                let length = count
                    .checked_mul(type_width(kind)?)
                    .context("TIFF: overflow campo")?;
                metadata_bytes = metadata_bytes
                    .checked_add(length)
                    .context("TIFF: overflow metadati")?;
                ensure!(
                    metadata_bytes <= 16 * 1024 * 1024,
                    "TIFF: metadati oltre quota"
                );
                let location = if length <= 4 {
                    entry + 8
                } else {
                    reader.long(entry + 8)? as usize
                };
                let data = reader.slice(location, length)?;
                ensure!(
                    fields
                        .insert(
                            tag,
                            Field {
                                kind,
                                count,
                                data,
                                le
                            }
                        )
                        .is_none(),
                    "TIFF: tag duplicato {tag}"
                );
            }
            let ifd = Ifd { fields };
            // SubIFDs only. EXIF/MakerNote offsets are not generic image IFDs.
            if let Some(sub) = ifd.get(330) {
                let sub = sub.integers(32)?;
                ensure!(
                    pending.len() + sub.len() <= 32,
                    "TIFF: troppe sotto-directory"
                );
                pending.extend(sub.into_iter().rev().map(|v| v as usize));
            }
            let next = reader.long(at + 2 + 12 * count)? as usize;
            if next != 0 {
                pending.push(next);
            }
            reader.ifds.push(ifd);
        }
        ensure!(!reader.ifds.is_empty(), "TIFF: nessuna directory");
        Ok(reader)
    }
    pub fn slice(&self, at: usize, count: usize) -> Result<&'a [u8]> {
        let end = at.checked_add(count).context("TIFF: overflow intervallo")?;
        self.bytes.get(at..end).context("TIFF: file troncato")
    }
    fn short(&self, at: usize) -> Result<u16> {
        let b = self.slice(at, 2)?.try_into().unwrap();
        Ok(if self.le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    fn long(&self, at: usize) -> Result<u32> {
        let b = self.slice(at, 4)?.try_into().unwrap();
        Ok(if self.le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
}
