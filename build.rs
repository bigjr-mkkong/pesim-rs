use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let dramsim_dir = PathBuf::from(&manifest_dir).join("third-party/DRAMsim3");
    let build_dir = dramsim_dir.join("build");

    //Build DRAMsim3
    std::fs::create_dir_all(&build_dir).expect("Failed to create build directory");
    let cmake_status = Command::new("cmake")
        .arg("-DCMAKE_POSITION_INDEPENDENT_CODE=ON")
        .arg("..")
        .current_dir(&build_dir)
        .status()
        .expect("Failed to execute CMake");

    if !cmake_status.success() {
        panic!("CMake failed to configure DRAMsim3");
    }

    let make_status = Command::new("make")
        .arg("-j")
        .current_dir(&build_dir)
        .status()
        .expect("Failed to execute Make");

    if !make_status.success() {
        panic!("Make failed to build DRAMsim3");
    }

    //link with libdramsim3.a
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-lib=static=dramsim3");

    println!("cargo:rerun-if-changed=src/dramsim3_cxx_ffi.rs");
    println!("cargo:rerun-if-changed=third-party/DRAMsim3/src");
    println!("cargo:rerun-if-changed=third-party/DRAMsim3/CMakeLists.txt");
}
