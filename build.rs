fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = root.join("app.manifest");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );
    println!("cargo:rerun-if-changed=assets/logo.ico");

    // The Windows resource group references the PNG images in the ICO directory.
    let ico = std::fs::read(root.join("assets/logo.ico")).expect("read application icon");
    let count = u16::from_le_bytes(ico[4..6].try_into().unwrap());
    let mut group = ico[..6].to_vec();
    let mut entries = vec![(0_u16, 0_u16, Vec::new())];
    for index in 0..count {
        let start = 6 + index as usize * 16;
        let entry = &ico[start..start + 16];
        let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        entries.push((3, index + 1, ico[offset..offset + length].to_vec()));
        group.extend_from_slice(&entry[..12]);
        group.extend_from_slice(&(index + 1).to_le_bytes());
    }
    entries.push((14, 1, group));
    let mut resource = Vec::new();
    for (kind, id, data) in entries {
        resource.extend_from_slice(&(data.len() as u32).to_le_bytes());
        resource.extend_from_slice(&32_u32.to_le_bytes());
        let flags = if kind == 0 { 0 } else { 0x1030 };
        for word in [0xffff, kind, 0xffff, id, 0, 0, flags, 0, 0, 0, 0, 0] {
            resource.extend_from_slice(&word.to_le_bytes());
        }
        resource.extend_from_slice(&data);
        resource.resize((resource.len() + 3) & !3, 0);
    }
    let resource_path =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("logo.res");
    std::fs::write(&resource_path, resource).expect("write application icon resource");
    println!("cargo:rustc-link-arg-bins={}", resource_path.display());
}
