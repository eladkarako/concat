use std::{
    env::var,
    path::PathBuf,
};
use winres::WindowsResource;

fn main() {
    let target_os = var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let path_base: PathBuf = var("CARGO_MANIFEST_DIR").unwrap().into();
    let cargo_toml_path = path_base.join("Cargo.toml");
    let cargo_toml_content = std::fs::read_to_string(&cargo_toml_path)
        .expect("Failed to read Cargo.toml");
    let cargo_toml: toml::Value = toml::from_str(&cargo_toml_content)
        .expect("Failed to parse Cargo.toml");

    // Extract metadata from [package]
    let package = &cargo_toml["package"];

    let name = package["name"]
        .as_str()
        .unwrap_or("unknown");
    let version = package["version"]
        .as_str()
        .unwrap_or("0.0.0");
    let description = package["description"]
        .as_str()
        .unwrap_or("");
    let repository = package["repository"]
        .as_str()
        .unwrap_or("");
    let author: &str = package["authors"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    // Parse version into major.minor.patch.release, and shifting each into its own byte range in 64-bit integer then joining them for winres.
    let version_parts: Vec<&str> = version.split('.').collect();
    let major: u64 = version_parts.get(0).and_then(|v| v.parse().ok()).unwrap_or(0);
    let minor: u64 = version_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    let patch: u64 = version_parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let release: u64 = version_parts.get(3).and_then(|v| v.parse().ok()).unwrap_or(0);
    let packed = (major << 48) | (minor << 32) | (patch << 16) | release;

    let path_icon = path_base.join("resources").join("app.ico");
    let path_manifest = path_base.join("resources").join("app.manifest");
    let mut res = WindowsResource::new();
    res.set_icon(path_icon.to_string_lossy().as_ref());
    res.set_manifest_file(path_manifest.to_string_lossy().as_ref());

    let lang_english: u16 = 0x09;                                 //primary language
    let sublang_english_us: u16 = 0x01;                           //sublanguage
    let langid: u16 = (sublang_english_us << 10) | lang_english;  // 0x0409 = English (United States) - C:\Program Files (x86)\Windows Kits\10\Include\10.0.28000.0\um\winnt.h
    res.set_language(langid);

    // Build VERSIONINFO from Cargo.toml metadata
    res.set("Comments", repository);
    res.set("FileDescription", description);
    res.set("InternalName", &format!("{}.exe", name));
    res.set("OriginalFilename", &format!("{}.exe", name));
    res.set("CompanyName", author);
    res.set("LegalCopyright", &format!("{}/LICENSE", repository));
    res.set("ProductName", name);

    let version_string = format!("{}.{}.{}.{}", major, minor, patch, release);
    res.set("FileVersion", &version_string);
    res.set("ProductVersion", &version_string);

    res.set_version_info(winres::VersionInfo::FILEVERSION, packed);
    res.set_version_info(winres::VersionInfo::PRODUCTVERSION, packed);


    // note: only if you need '#include <windows.h>' in the .rc file, uncomment the following lines (modify path to where to find windows.h on your computer). needs to be before .compile() or calls to 'rc.exe' .
    // let include_value = r"C:\Program Files (x86)\Windows Kits\10\Include\10.0.28000.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.28000.0\shared";
    // unsafe { set_var("INCLUDE", include_value); }
    // println!("cargo:rustc-link-search=native={}", include_value);

    res.compile().unwrap();
}
