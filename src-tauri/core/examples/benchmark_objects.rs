//! Disposable synthetic object benchmark. Does not inspect any user profile.
use sha2::{Digest, Sha256};
use spice_route_core::models::{ObjectEntry, ObjectKind};
use spice_route_core::snapshot::ObjectStore;
use spice_route_core::util::sha256_file;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;

fn legacy_capture(store: &ObjectStore, path: &Path) -> ObjectEntry {
    let temporary = path.with_extension("legacy-zst");
    let mut encoder =
        zstd::stream::write::Encoder::new(BufWriter::new(File::create(&temporary).unwrap()), 6)
            .unwrap();
    let mut source = BufReader::new(File::open(path).unwrap());
    let mut buffer = vec![0; 1024 * 1024];
    let mut hasher = Sha256::new();
    let mut size = 0;
    loop {
        let count = source.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        encoder.write_all(&buffer[..count]).unwrap();
        hasher.update(&buffer[..count]);
        size += count as u64;
    }
    let mut output = encoder.finish().unwrap();
    output.flush().unwrap();
    output.get_ref().sync_all().unwrap();
    drop(output);
    let hash = format!("{:x}", hasher.finalize());
    assert_eq!(sha256_file(path).unwrap(), (hash.clone(), size));
    let destination = store.object_path(&hash).unwrap();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::rename(temporary, &destination).unwrap();
    ObjectEntry {
        hash,
        logical_path: "fixture".into(),
        kind: ObjectKind::ProjectFile,
        owner_id: "fixture".into(),
        raw_size: size,
        stored_size: fs::metadata(destination).unwrap().len(),
        executable: false,
    }
}

fn main() {
    let root = tempfile::tempdir().unwrap();
    for binary in [false, true] {
        let workload = if binary {
            "64 MiB mixed binary"
        } else {
            "128 unique 8 KiB files"
        };
        let directory = root.path().join(if binary { "binary" } else { "small" });
        fs::create_dir_all(&directory).unwrap();
        let mut sources = Vec::new();
        let mut random = 0x715a_0dd1_u32;
        for index in 0..if binary { 1 } else { 128 } {
            let mut bytes = vec![0_u8; if binary { 64 * 1024 * 1024 } else { 8192 }];
            for (offset, byte) in bytes.iter_mut().enumerate() {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                *byte = if binary && offset % 4096 < 2048 {
                    random as u8
                } else {
                    (offset % 251) as u8
                };
            }
            bytes[..4].copy_from_slice(&(index as u32).to_le_bytes());
            let path = directory.join(format!("{index}.bin"));
            fs::write(&path, bytes).unwrap();
            sources.push(path);
        }
        let legacy = ObjectStore::new(directory.join("legacy-stage")).unwrap();
        let legacy_cloud = ObjectStore::new(directory.join("legacy-cloud")).unwrap();
        let start = Instant::now();
        let mut legacy_stored = 0;
        for path in &sources {
            let object = legacy_capture(&legacy, path);
            legacy.verify(&object).unwrap();
            let destination = legacy_cloud.object_path(&object.hash).unwrap();
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            let mut input = File::open(legacy.object_path(&object.hash).unwrap()).unwrap();
            let mut output = File::create(&destination).unwrap();
            std::io::copy(&mut input, &mut output).unwrap();
            output.sync_all().unwrap();
            drop(output);
            legacy_cloud.verify(&object).unwrap();
            legacy_stored += object.stored_size;
        }
        let old = start.elapsed();
        let cloud_path = directory.join("new-cloud");
        let current = ObjectStore::with_reuse(directory.join("new-stage"), &cloud_path).unwrap();
        let cloud = ObjectStore::new(&cloud_path).unwrap();
        let start = Instant::now();
        let mut current_stored = 0;
        for path in &sources {
            let object = current
                .put_file(
                    path,
                    "fixture".into(),
                    ObjectKind::ProjectFile,
                    "fixture".into(),
                )
                .unwrap();
            cloud.import_from(&current, &object).unwrap();
            current_stored += object.stored_size;
        }
        let fresh = start.elapsed();
        let repeated = ObjectStore::with_reuse(directory.join("reuse-stage"), &cloud_path).unwrap();
        let start = Instant::now();
        for path in &sources {
            let object = repeated
                .put_file(
                    path,
                    "fixture".into(),
                    ObjectKind::ProjectFile,
                    "fixture".into(),
                )
                .unwrap();
            cloud.import_from(&repeated, &object).unwrap();
        }
        let reused = start.elapsed();
        assert_eq!(
            fs::read_dir(directory.join("reuse-stage")).unwrap().count(),
            0
        );
        println!("{workload}: legacy {} ms; fresh {} ms; unchanged {} ms; compressed bytes legacy {legacy_stored}, current {current_stored}", old.as_millis(), fresh.as_millis(), reused.as_millis());
    }
}
