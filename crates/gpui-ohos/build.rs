//! OHOS 目标的链接参数。
//!
//! 构建脚本运行在宿主（Windows/Linux）上，所以目标平台必须看
//! `CARGO_CFG_TARGET_ENV` 而不是 `cfg(target_env = ...)`——后者描述的是宿主。
//!
//! 交叉脚本 `script/ohos_cross.sh` 会导出 `OHOS_NDK_HOME`（已做 8.3 短路径化，
//! 因为 cc-rs 按空白切分 CFLAGS）。这里只做两件事：
//!
//! 1. 把 NDK 的 `sysroot/usr/lib/<triple>` 与 `llvm/lib/<triple>` 加进链接搜索路径；
//! 2. 声明本 crate 真正用到的系统库。
//!
//! 非 OHOS 目标不做任何事——宿主 `cargo test` 不链接这些库。

fn main() {
    println!("cargo:rerun-if-env-changed=OHOS_NDK_HOME");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("ohos") {
        return;
    }

    let ndk = std::env::var("OHOS_NDK_HOME").unwrap_or_else(|_| {
        panic!(
            "gpui-ohos: OHOS_NDK_HOME is not set. Build OHOS targets through \
             `bash script/ohos_cross.sh <cargo command>`, which exports the NDK root \
             (the directory containing `sysroot/` and `llvm/`)."
        )
    });

    let target = std::env::var("TARGET").expect("gpui-ohos: TARGET is not set by cargo");
    let lib_triple = match target.as_str() {
        "aarch64-unknown-linux-ohos" => "aarch64-linux-ohos",
        "armv7-unknown-linux-ohos" => "arm-linux-ohos",
        "x86_64-unknown-linux-ohos" => "x86_64-linux-ohos",
        other => panic!(
            "gpui-ohos: unsupported OHOS target `{other}`; expected \
             aarch64-unknown-linux-ohos, armv7-unknown-linux-ohos or x86_64-unknown-linux-ohos"
        ),
    };

    let sysroot_lib = format!("{ndk}/sysroot/usr/lib/{lib_triple}");
    let llvm_lib = format!("{ndk}/llvm/lib/{lib_triple}");
    for dir in [&sysroot_lib, &llvm_lib] {
        if !std::path::Path::new(dir).is_dir() {
            panic!("gpui-ohos: expected OHOS library directory `{dir}` does not exist");
        }
        println!("cargo:rustc-link-search=native={dir}");
    }

    // EGL/GLESv3：`WgpuRenderer` 的 GL 后端经 libEGL 动态加载入口，但
    // eglGetProcAddress 解析到的 GLES 符号需要进程已映射 libGLESv3。
    // ace_napi.z：Node-API 导出面。
    // native_window：OHNativeWindow 几何查询。
    // native_vsync：vsync 帧源。
    for lib in ["EGL", "GLESv3", "ace_napi.z", "native_window", "native_vsync"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }

    // 让 `napi-derive-ohos` 生成的模块名与本 crate 的 .so 名一致（gpui_ohos）。
    // 不设置时 derive 会退回到 CARGO_CRATE_NAME，结果相同；显式写出来是为了
    // 让 ArkTS 侧 `libraryname: 'gpui_ohos'` 有唯一事实来源。
    println!("cargo:rustc-env=NAPI_BUILD_TARGET_NAME=gpui_ohos");

    // napi-build-ohos 还会补上 NDK 的链接参数（含 libc++_shared 的搜索路径）。
    napi_build_ohos::setup();
}
