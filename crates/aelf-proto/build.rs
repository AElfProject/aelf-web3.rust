use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto_root = manifest_dir.join("proto/upstream");
    let mut files = collect_proto_files(&proto_root)?;
    files.sort();

    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let descriptor_path = out_dir.join("aelf_descriptor.bin");

    let mut config = prost_build::Config::new();
    config.file_descriptor_set_path(&descriptor_path);
    config.compile_well_known_types();
    config.extern_path(".google.protobuf", "::pbjson_types");
    config.include_file("_includes.rs");
    config.compile_protos(&files, &[proto_root])?;

    let descriptor_bytes = fs::read(&descriptor_path)?;
    let packages = collect_packages(&descriptor_bytes)?;
    pbjson_build::Builder::new()
        .register_descriptors(&descriptor_bytes)?
        .build(&packages)?;

    Ok(())
}

fn collect_proto_files(root: &PathBuf) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    collect_proto_files_inner(root, &mut files)?;
    Ok(files)
}

fn collect_proto_files_inner(
    root: &PathBuf,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_proto_files_inner(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("proto") {
            files.push(path);
        }
    }

    Ok(())
}

fn collect_packages(descriptor_bytes: &[u8]) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use prost::Message;
    use prost_types::FileDescriptorSet;
    use std::collections::BTreeSet;

    let descriptor = FileDescriptorSet::decode(descriptor_bytes)?;
    let packages = descriptor
        .file
        .into_iter()
        .filter_map(|file| file.package)
        .filter(|package| !package.is_empty())
        .map(|package| format!(".{package}"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(packages)
}
