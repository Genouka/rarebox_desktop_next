//! Node-API 绑定层。
//!
//! ArkTS 侧 `import gpui_ohos from 'libgpui_ohos.so'`（`libraryname: 'gpui_ohos'`），
//! 导出名见 [`api`]。模块名必须与 `.so` 名一致，否则 ArkUI 加载时会崩溃
//! （`napi-derive-ohos` 的注释），`build.rs` 通过 `NAPI_BUILD_TARGET_NAME` 固定。
//!
//! # 线程
//!
//! 所有导出函数运行在 ArkTS/UI 线程；它们只解析 JSON、投递命令、唤醒渲染线程。
//! 唯一例外是 `detach_surface`，它**必须**阻塞到渲染线程放弃 surface 指针
//! （风险 R10）。事件方向一律经 `ThreadsafeFunction`（NonBlocking）。

pub mod api;
pub mod xcomponent;

use napi_ohos::bindgen_prelude::sys;
use std::{
    ffi::{c_void, CStr},
    ptr,
};

/// `OH_NATIVE_XCOMPONENT_OBJ`（`ace/xcomponent/native_interface_xcomponent.h:59`）。
const XCOMPONENT_EXPORT: &CStr = c"__NATIVE_XCOMPONENT_OBJ__";

/// 模块初始化时从 exports 取 `OH_NativeXComponent*` 并注册全部回调。
///
/// `napi-derive-ohos` 用 `module_init` 生成 `.init_array` 构造函数，早于
/// `napi_register_module_v1` 执行，因此 hook 在导出表建好之后、ArkTS 调用任何
/// 导出函数之前被调用（`module_register.rs` 的 `MODULE_REGISTER_HOOK_CALLBACK`）。
#[napi_derive_ohos::module_init]
fn install_xcomponent_hook() {
    // 在模块注册过程中登记静态 hook，进程内只做一次。
    napi_ohos::bindgen_prelude::register_module_export_hook(read_xcomponent_export);
}

/// SAFETY: `env`/`exports` 由 Node-API 在模块注册时提供。
unsafe fn read_xcomponent_export(
    env: sys::napi_env,
    exports: sys::napi_value,
) -> napi_ohos::Result<sys::napi_value> {
    let mut object: sys::napi_value = ptr::null_mut();
    // SAFETY: env/exports 有效，属性名是静态 NUL 结尾字符串。
    let status = unsafe {
        sys::napi_get_named_property(env, exports, XCOMPONENT_EXPORT.as_ptr(), &mut object)
    };
    if status != 0 {
        // 宿主没有用 XComponent（例如纯 surface id 模式）时不报错。
        log::debug!(
            "XComponent: __NATIVE_XCOMPONENT_OBJ__ missing from exports (status={status}); \
             attach_surface(surfaceId) remains available"
        );
        return Ok(exports);
    }

    let mut data: *mut c_void = ptr::null_mut();
    // SAFETY: object 是 external，napi_unwrap 把它解成注册时的指针。
    let status = unsafe { sys::napi_unwrap(env, object, &mut data) };
    if status != 0 {
        log::warn!("XComponent: napi_unwrap failed (status={status})");
        return Ok(exports);
    }
    xcomponent::install(data);
    Ok(exports)
}
