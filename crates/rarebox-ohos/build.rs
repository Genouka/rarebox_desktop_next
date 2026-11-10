//! Node-API 构建辅助。
//!
//! 构建脚本运行在宿主（如 Windows）上，所以目标平台必须看
//! `CARGO_CFG_TARGET_ENV` 而不是 `cfg(target_env = ...)`——后者描述的是宿主。
//! 只有交叉编译到 OHOS 时才需要 `napi_build_ohos::setup()` 注入 NDK 链接参数
//! （`OHOS_NDK_HOME` 必须指向 SDK 的 `native` 目录）。

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos") {
        napi_build_ohos::setup();
    }
}
