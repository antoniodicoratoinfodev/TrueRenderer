//! Versions 1/2: bounded JSON manifest, little-endian fp32 assets, SHA-256 footer.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};
use tr_core::document::{Document, MAX_CANVAS_PIXELS, MAX_DOCUMENT_BYTES, MAX_LAYERS};
const MAGIC_V1: &[u8; 8] = b"TRDOC\0\x01\0";
const MAGIC_V2: &[u8; 8] = b"TRDOC\0\x02\0";
const MAX_MANIFEST: usize = 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    document: Document,
    selection: bool,
    masks: Vec<bool>,
}

pub fn write(writer: &mut impl Write, document: &Document) -> Result<()> {
    document.validate()?;
    // The manifest serializer omits all raster arrays.
    #[derive(Serialize)]
    struct Borrowed<'a> {
        document: &'a Document,
        selection: bool,
        masks: Vec<bool>,
    }
    let manifest = serde_json::to_vec(&Borrowed {
        document,
        selection: !document.selection.is_empty(),
        masks: document.layers.iter().map(|l| !l.mask.is_empty()).collect(),
    })?;
    ensure!(manifest.len() <= MAX_MANIFEST, "Manifest fuori quota");
    let mut hash = Sha256::new();
    let mut block = |bytes: &[u8]| -> Result<()> {
        writer.write_all(bytes)?;
        hash.update(bytes);
        Ok(())
    };
    block(if document.version == 1 {
        MAGIC_V1
    } else {
        MAGIC_V2
    })?;
    block(&(manifest.len() as u32).to_le_bytes())?;
    block(&manifest)?;
    let mut buffer = Vec::with_capacity(64 * 1024);
    for values in std::iter::once(document.selection.as_slice()).chain(
        document
            .layers
            .iter()
            .flat_map(|l| [bytemuck::cast_slice(l.pixels.as_slice()), l.mask.as_slice()]),
    ) {
        for chunk in values.chunks(16384) {
            buffer.clear();
            for v in chunk {
                buffer.extend_from_slice(&v.to_le_bytes());
            }
            block(&buffer)?;
        }
    }
    writer.write_all(&hash.finalize())?;
    Ok(())
}

pub fn read(reader: &mut impl Read) -> Result<Document> {
    read_admitted(reader, u64::MAX)
}
fn read_admitted(reader: &mut impl Read, maximum: u64) -> Result<Document> {
    let mut hash = Sha256::new();
    let mut block = |bytes: &mut [u8]| -> Result<()> {
        reader.read_exact(bytes)?;
        hash.update(&*bytes);
        Ok(())
    };
    let mut magic = [0; 8];
    block(&mut magic)?;
    ensure!(
        &magic == MAGIC_V1 || &magic == MAGIC_V2,
        "Non è un documento TrueRenderer supportato"
    );
    let mut size = [0; 4];
    block(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    ensure!(size <= MAX_MANIFEST, "Manifest fuori quota");
    let mut bytes = vec![0; size];
    block(&mut bytes)?;
    let Manifest {
        mut document,
        selection,
        masks,
    } = serde_json::from_slice::<Manifest>(&bytes)?;
    ensure!(
        document.version == magic[6] as u32
            && document.layers.len() <= MAX_LAYERS
            && masks.len() == document.layers.len(),
        "Versione/livelli non validi"
    );
    let count = |w: u32, h: u32| -> Result<usize> {
        let n = u64::from(w) * u64::from(h);
        ensure!(
            w > 0 && h > 0 && n <= MAX_CANVAS_PIXELS as u64,
            "Dimensioni fuori quota"
        );
        Ok(n as usize)
    };
    let n = count(document.width, document.height)?;
    let counts = document
        .layers
        .iter()
        .map(|l| count(l.width, l.height))
        .collect::<Result<Vec<_>>>()?;
    let total = if selection { n * 4 } else { 0 }
        + counts
            .iter()
            .zip(&masks)
            .map(|(n, m)| n * (16 + if *m { 4 } else { 0 }))
            .sum::<usize>();
    ensure!(total <= MAX_DOCUMENT_BYTES, "Documento fuori quota memoria");
    let largest = counts
        .iter()
        .copied()
        .chain(std::iter::once(n))
        .max()
        .unwrap_or(n);
    ensure!(
        tr_core::document::admission_bytes(total as u64, largest as u64) <= maximum,
        "Documento cambiato o oltre la memoria ammessa"
    );
    let mut floats = |n: usize| -> Result<Vec<f32>> {
        let mut values = Vec::new();
        values.try_reserve_exact(n)?;
        let mut buf = [0u8; 65536];
        let mut remaining = n;
        while remaining > 0 {
            let take = remaining.min(16384);
            block(&mut buf[..take * 4])?;
            values.extend(
                buf[..take * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b)),
            );
            remaining -= take;
        }
        Ok(values)
    };
    if selection {
        document.selection = floats(n)?;
    }
    for ((layer, n), mask) in document.layers.iter_mut().zip(counts).zip(masks) {
        // Allocate the final pixel buffer directly, avoiding a second full raster.
        layer.pixels.try_reserve_exact(n)?;
        let mut remaining = n;
        while remaining > 0 {
            let take = remaining.min(4096);
            let values = floats(take * 4)?;
            layer
                .pixels
                .extend(values.as_chunks::<4>().0.iter().copied());
            remaining -= take;
        }
        if mask {
            layer.mask = floats(n)?;
        }
    }
    let actual = hash.finalize();
    let mut expected = [0; 32];
    reader.read_exact(&mut expected)?;
    ensure!(actual[..] == expected, "Checksum documento non valido");
    let mut tail = [0];
    ensure!(
        reader.read(&mut tail)? == 0,
        "Dati aggiuntivi dopo il documento"
    );
    document.validate()?;
    Ok(document)
}
/// Validate the bounded manifest before the host admits raster allocations.
pub fn allocation(path: &Path) -> Result<(u64, u64)> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    ensure!(
        &magic == MAGIC_V1 || &magic == MAGIC_V2,
        "Versione documento non supportata"
    );
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    ensure!(size <= MAX_MANIFEST, "Manifest fuori quota");
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let d = &manifest.document;
    ensure!(
        d.version == magic[6] as u32
            && d.layers.len() <= MAX_LAYERS
            && manifest.masks.len() == d.layers.len(),
        "Manifest documento non valido"
    );
    let area = |w: u32, h: u32| -> Result<u64> {
        let n = u64::from(w) * u64::from(h);
        ensure!(
            w > 0 && h > 0 && n <= MAX_CANVAS_PIXELS as u64,
            "Dimensioni fuori quota"
        );
        Ok(n)
    };
    let mut largest = area(d.width, d.height)?;
    let mut storage = if manifest.selection { largest * 4 } else { 0 };
    for (layer, mask) in d.layers.iter().zip(manifest.masks) {
        let n = area(layer.width, layer.height)?;
        largest = largest.max(n);
        storage += n * if mask { 20 } else { 16 };
    }
    ensure!(
        storage <= MAX_DOCUMENT_BYTES as u64,
        "Documento fuori quota memoria"
    );
    Ok((storage, largest))
}
pub fn open(path: &Path) -> Result<Document> {
    open_admitted(path, u64::MAX)
}
pub fn open_admitted(path: &Path, maximum: u64) -> Result<Document> {
    read_admitted(&mut BufReader::new(File::open(path)?), maximum)
}

pub fn save(path: &Path, document: &Document) -> Result<()> {
    save_mode(path, document, false)
}
/// Publish a new project atomically without replacing any existing file.
pub fn create(path: &Path, document: &Document) -> Result<()> {
    save_mode(path, document, true)
}
fn save_mode(path: &Path, document: &Document, create: bool) -> Result<()> {
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("truerenderer")),
        "Usare l'estensione .truerenderer"
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix(".tr-document-")
        .tempfile_in(parent)?;
    {
        let mut writer = BufWriter::new(temp.as_file_mut());
        write(&mut writer, document)?;
        writer.flush()?;
    }
    temp.as_file().sync_all()?;
    if !create && path.exists() {
        // Never replace a photograph or unrelated file, even with a misleading extension.
        open(path).context("Il documento esistente non è valido: scegliere Salva con nome")?;
        let backup = path.with_file_name(format!(
            "{}.previous-{}.truerenderer",
            path.file_name().unwrap_or_default().to_string_lossy(),
            uuid::Uuid::new_v4()
        ));
        let mut previous = tempfile::Builder::new()
            .prefix(".tr-previous-")
            .tempfile_in(parent)?;
        std::io::copy(&mut File::open(path)?, previous.as_file_mut())?;
        previous.as_file().sync_all()?;
        previous.persist_noclobber(&backup).map_err(|e| e.error)?;
    }
    if create {
        temp.persist_noclobber(path).map_err(|e| e.error)?;
    } else {
        temp.persist(path).map_err(|e| e.error)?;
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_v1_and_native_v2_density_roundtrip_with_admission() {
        let mut doc = Document::new(8, 6, 240., [0.; 4]).unwrap();
        doc.version = 1;
        doc.resolution = None;
        let mut bytes = vec![];
        write(&mut bytes, &doc).unwrap();
        assert_eq!(&bytes[..8], MAGIC_V1);
        let legacy = read(&mut bytes.as_slice()).unwrap();
        assert_eq!(
            legacy.physical_resolution(),
            Some(tr_core::protocol::Resolution { x: 240., y: 240. })
        );
        for resolution in [
            None,
            Some(tr_core::protocol::Resolution { x: 240., y: 150. }),
        ] {
            doc.version = 2;
            doc.resolution = resolution;
            let mut bytes = vec![];
            write(&mut bytes, &doc).unwrap();
            assert_eq!(&bytes[..8], MAGIC_V2);
            assert_eq!(read(&mut bytes.as_slice()).unwrap(), doc);
            assert!(read_admitted(&mut bytes.as_slice(), 1024).is_err());
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("native.truerenderer");
            save(&path, &doc).unwrap();
            let (storage, pixels) = allocation(&path).unwrap();
            assert_eq!((storage, pixels), (8 * 6 * 16, 8 * 6));
            let admitted = tr_core::document::admission_bytes(storage, pixels);
            assert_eq!(open_admitted(&path, admitted).unwrap(), doc);
        }
    }
    #[test]
    fn portable_roundtrip_and_corruption() {
        let mut doc = Document::new(3, 2, 300., [0.2, 0.1, 0., 0.5]).unwrap();
        doc.duplicate(0).unwrap();
        doc.layers[1].pixels[0] = [0.; 4];
        doc.layers[1].transform.x = 0.5;
        doc.layers[1].recipe.exposure_ev = 0.7;
        doc.layers[1]
            .recipe
            .set_section_enabled("light-panel", false)
            .unwrap();
        doc.selection = vec![0.25; 6];
        doc.layers[0].mask = vec![0.75; 6];
        let mut bytes = vec![];
        write(&mut bytes, &doc).unwrap();
        assert_eq!(&bytes[..8], MAGIC_V2);
        assert_eq!(read(&mut bytes.as_slice()).unwrap(), doc);
        assert_eq!(
            read(&mut bytes.as_slice())
                .unwrap()
                .render()
                .unwrap()
                .pixels,
            doc.render().unwrap().pixels
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("città レイヤー.truerenderer");
        save(&path, &doc).unwrap();
        let old = doc.clone();
        doc.layers[0].opacity = 0.3;
        save(&path, &doc).unwrap();
        assert_eq!(open(&path).unwrap(), doc);
        let backup = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".previous-")
            })
            .unwrap();
        assert_eq!(open(&backup).unwrap(), old);
        let mut invalid = doc.clone();
        invalid.layers[0].transform.scale = 0.;
        assert!(save(&path, &invalid).is_err());
        assert_eq!(open(&path).unwrap(), doc);
        for n in [0, 7, 12, bytes.len() - 1] {
            assert!(read(&mut &bytes[..n]).is_err());
        }
        let i = bytes.len() - 33;
        bytes[i] ^= 1;
        assert!(read(&mut bytes.as_slice()).is_err());
    }
}
