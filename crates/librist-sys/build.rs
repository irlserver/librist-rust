//! Build script for librist-sys
//!
//! This script handles:
//! 1. Building librist from source (bundled feature)
//! 2. Using system librist (system feature)
//! 3. Generating Rust bindings via bindgen

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    // Determine build mode
    let use_bundled = cfg!(feature = "bundled") && !cfg!(feature = "system");
    let use_static = cfg!(feature = "static") || target_os == "macos";

    let (include_path, lib_path) = if use_bundled {
        build_librist(&out_dir, &target_os, &target_arch, use_static)
    } else {
        find_system_librist()
    };

    // Configure linking
    configure_linking(&lib_path, &target_os, use_static);

    // Generate bindings
    generate_bindings(&include_path, &out_dir);

    // Rerun triggers
    println!("cargo:rerun-if-changed=librist/");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=LIBRIST_DIR");
    println!("cargo:rerun-if-env-changed=LIBRIST_LIB_DIR");
}

/// Build librist from bundled source using Meson
fn build_librist(
    out_dir: &Path,
    _target_os: &str,
    _target_arch: &str,
    use_static: bool,
) -> (PathBuf, PathBuf) {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Find librist source directory - check submodule first
    let librist_src = manifest_dir.join("librist");

    if !librist_src.exists() || !librist_src.join("meson.build").exists() {
        panic!(
            "librist source not found at {:?}. Please initialize git submodules with:\n\
             git submodule update --init --recursive",
            librist_src
        );
    }

    let build_dir = out_dir.join("librist-build");
    let install_dir = out_dir.join("librist-install");

    // Skip build if already done
    if install_dir.join("lib").exists() || install_dir.join("lib64").exists() {
        return get_install_paths(&install_dir);
    }

    println!("cargo:warning=Building librist from source...");

    // Prepare meson arguments
    let mut meson_args = vec![
        "setup".to_string(),
        build_dir.to_string_lossy().to_string(),
        format!("--prefix={}", install_dir.display()),
        "--buildtype=release".to_string(),
        "-Dbuilt_tools=false".to_string(),
        "-Dtest=false".to_string(),
    ];

    if use_static {
        meson_args.push("--default-library=static".to_string());
    }

    // Configure meson options based on features
    // Use builtin dependencies for easier static linking
    meson_args.push("-Dbuiltin_cjson=true".to_string());
    meson_args.push("-Dfallback_builtin=true".to_string());

    // mbedTLS provides AES encryption and SRP authentication
    // Enabled by default, can be disabled with default-features = false
    if cfg!(feature = "mbedtls") {
        meson_args.push("-Duse_mbedtls=true".to_string());
        meson_args.push("-Dbuiltin_mbedtls=true".to_string());
    } else {
        meson_args.push("-Duse_mbedtls=false".to_string());
    }

    // Run meson setup
    run_command_vec("meson", &meson_args, &librist_src, "meson setup");

    // Run meson compile
    run_command(
        "meson",
        &["compile", "-C", build_dir.to_str().unwrap()],
        &librist_src,
        "meson compile",
    );

    // Run meson install
    run_command(
        "meson",
        &["install", "-C", build_dir.to_str().unwrap()],
        &librist_src,
        "meson install",
    );

    get_install_paths(&install_dir)
}

/// Get include and lib paths from install directory
fn get_install_paths(install_dir: &Path) -> (PathBuf, PathBuf) {
    let include_path = install_dir.join("include");

    // Handle different distro library paths
    let lib_path = if install_dir.join("lib64").exists() {
        install_dir.join("lib64")
    } else if install_dir.join("lib").join("x86_64-linux-gnu").exists() {
        install_dir.join("lib").join("x86_64-linux-gnu")
    } else {
        install_dir.join("lib")
    };

    (include_path, lib_path)
}

/// Find system-installed librist using pkg-config
fn find_system_librist() -> (PathBuf, PathBuf) {
    // Check for environment variable override
    if let Ok(librist_dir) = env::var("LIBRIST_DIR") {
        let path = PathBuf::from(librist_dir);
        return (path.join("include"), path.join("lib"));
    }

    // Try pkg-config
    let lib = pkg_config::Config::new()
        .atleast_version("0.2.0")
        .probe("librist")
        .expect("librist not found. Install librist or use the 'bundled' feature.");

    let include_path = lib
        .include_paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/usr/include"));

    let lib_path = lib
        .link_paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/usr/lib"));

    (include_path, lib_path)
}

/// Configure library linking
fn configure_linking(lib_path: &Path, target_os: &str, use_static: bool) {
    println!("cargo:rustc-link-search=native={}", lib_path.display());

    match target_os {
        "windows" => {
            if use_static {
                println!("cargo:rustc-link-lib=static=rist");
            } else {
                println!("cargo:rustc-link-lib=dylib=librist");
                // Copy DLL for runtime
                copy_windows_dll(lib_path);
            }
            // Windows socket libraries
            println!("cargo:rustc-link-lib=ws2_32");
        }
        "macos" => {
            if use_static {
                println!("cargo:rustc-link-lib=static=rist");
            } else {
                println!("cargo:rustc-link-lib=dylib=rist");
            }
            // macOS frameworks
            println!("cargo:rustc-link-lib=framework=Security");
        }
        _ => {
            // Linux/Unix
            if use_static {
                println!("cargo:rustc-link-lib=static=rist");
                // Static linking may require additional libraries
                println!("cargo:rustc-link-lib=pthread");
                println!("cargo:rustc-link-lib=m");
            } else {
                println!("cargo:rustc-link-lib=dylib=rist");
            }
        }
    }

    // Note: When using builtin_mbedtls=true (which we do), mbedtls is statically
    // linked into librist, so we don't need to link it separately.
    // Only link mbedtls separately if using system mbedtls.
}

/// Generate Rust bindings using bindgen
fn generate_bindings(include_path: &Path, out_dir: &Path) {
    let wrapper_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("wrapper.h");

    // If wrapper.h doesn't exist, create it
    if !wrapper_path.exists() {
        std::fs::write(
            &wrapper_path,
            r#"// Wrapper header for librist bindings
#include <librist/librist.h>
#include <librist/udpsocket.h>
"#,
        )
        .expect("Failed to create wrapper.h");
    }

    let bindings = bindgen::Builder::default()
        .header(wrapper_path.to_string_lossy())
        // Include paths
        .clang_arg(format!("-I{}", include_path.display()))
        // Enum handling
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: true,
        })
        .bitfield_enum("rist_data_block_sender_flags")
        .bitfield_enum("rist_data_block_receiver_flags")
        // Derive traits
        .derive_debug(true)
        .derive_default(true)
        .derive_copy(true)
        .derive_eq(true)
        .derive_hash(true)
        .derive_partialeq(true)
        // Don't derive Eq/Hash/PartialEq for types with function pointers
        // (comparing function pointers is meaningless and triggers warnings)
        .no_partialeq("__sFILE")
        .no_hash("__sFILE")
        .no_partialeq("rist_logging_settings")
        .no_hash("rist_logging_settings")
        .no_partialeq("rist_thread_callback_t")
        .no_hash("rist_thread_callback_t")
        // Function allowlist
        .allowlist_function("rist_.*")
        .allowlist_function("librist_.*")
        .allowlist_function("udpsocket_.*")
        .allowlist_function("evsocket_.*")
        // Type allowlist
        .allowlist_type("rist_.*")
        .allowlist_type("librist_.*")
        .allowlist_type("udpsocket_.*")
        .allowlist_type("evsocket_.*")
        // Variable allowlist
        .allowlist_var("RIST_.*")
        .allowlist_var("LIBRIST_.*")
        .allowlist_var("EVSOCKET_.*")
        // Blocklist deprecated functions (use *2 variants)
        .blocklist_function("rist_receiver_data_callback_set$")
        .blocklist_function("rist_receiver_data_read$")
        .blocklist_function("rist_parse_address$")
        .blocklist_function("rist_peer_config_free$")
        .blocklist_function("rist_udp_config_free$")
        .blocklist_function("rist_logging_settings_free$")
        // Generate comments
        .generate_comments(true)
        .clang_arg("-fparse-all-comments")
        // Layout tests
        .layout_tests(true)
        // Use core types
        .use_core()
        // Handle FILE* type
        .blocklist_type("FILE")
        .raw_line("pub type FILE = libc::FILE;")
        .generate()
        .expect("Failed to generate bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Failed to write bindings");
}

/// Run a command and panic on failure
fn run_command(program: &str, args: &[&str], cwd: &Path, description: &str) {
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap_or_else(|e| panic!("Failed to execute {}: {}", description, e));

    if !status.success() {
        panic!("{} failed with status: {}", description, status);
    }
}

/// Run a command with Vec<String> args and panic on failure
fn run_command_vec(program: &str, args: &[String], cwd: &Path, description: &str) {
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap_or_else(|e| panic!("Failed to execute {}: {}", description, e));

    if !status.success() {
        panic!("{} failed with status: {}", description, status);
    }
}

/// Copy Windows DLL for runtime (Windows only)
#[cfg(target_os = "windows")]
fn copy_windows_dll(lib_path: &Path) {
    let dll_name = "librist.dll";
    let src = lib_path.join(dll_name);
    if src.exists() {
        if let Ok(out_dir) = env::var("OUT_DIR") {
            let dst = PathBuf::from(out_dir).join(dll_name);
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn copy_windows_dll(_lib_path: &Path) {}
