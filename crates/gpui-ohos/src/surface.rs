//! surface 抽象：尺寸推导 + `raw-window-handle` 包装。
//!
//! wgpu 只需要两样东西就能在鸿蒙上建 surface：
//!
//! 1. `RawWindowHandle::OhosNdk(OhosNdkWindowHandle { native_window })`
//!    —— `OHNativeWindow*` 指针来自 XComponent 的 `OnSurfaceCreated(component, window)`；
//! 2. `RawDisplayHandle::Ohos(OhosDisplayHandle::new())` —— 无字段。
//!
//! 有了这两者，`gpui_wgpu::WgpuRenderer::new` 不需要任何改动：它的泛型约束
//! `W: HasWindowHandle + HasDisplayHandle + Debug + Send + Sync + Clone + 'static`
//! 由 [`RawOhosWindow`] 满足，wgpu-hal 的 EGL 后端有 `Rwh::OhosNdk` 分支
//! （`gles/egl.rs:1252`）。
//!
//! 尺寸有两个来源，按可靠性排序：
//!
//! * `OH_NativeXComponent_GetXComponentSize(component, window, &w, &h)`：**单位 vp**，
//!   需要 XComponent 句柄（模块初始化时 `napi_unwrap` 拿到）；
//! * `OH_NativeWindow_NativeWindowHandleOpt(window, GET_BUFFER_GEOMETRY, &h, &w)`：
//!   **单位 px**，注意头文件的参数顺序是 `height, width`。
//!
//! 宿主也会通过 NAPI `resize(widthPx, heightPx)` 主动报尺寸，作为最终兜底。


/// 把任意来源的尺寸收敛成 wgpu 能配置的值（至少 1×1）。
///
/// 0×0 在 XComponent 尚未布局时很常见；`eglCreateWindowSurface` 配 0 尺寸
/// 会得到无效的 swapchain，之后所有 `draw` 都失败，表现为黑屏而不是日志。
pub fn sanitize_size(width: u32, height: u32) -> (u32, u32) {
    (width.max(1), height.max(1))
}

/// 由 vp 尺寸与密度得到 surface 的物理像素尺寸。
pub fn surface_size_from_vp(width_vp: f32, height_vp: f32, density: f32) -> (u32, u32) {
    let (width, height) = crate::device_size_from_vp(width_vp, height_vp, density);
    sanitize_size(width, height)
}

/// 安全区/键盘避让。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InsetsPx {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl InsetsPx {
    pub fn new(top: i32, right: i32, bottom: i32, left: i32) -> Self {
        Self {
            top: top.max(0),
            right: right.max(0),
            bottom: bottom.max(0),
            left: left.max(0),
        }
    }

    /// 与另一组避让取逐边最大值（安全区与键盘可能重叠）。
    pub fn max(self, other: Self) -> Self {
        Self {
            top: self.top.max(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
            left: self.left.max(other.left),
        }
    }

    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    /// 转成 GPUI 的逻辑像素边距。
    pub fn to_edges(self, density: f32) -> gpui::Edges<gpui::Pixels> {
        gpui::Edges {
            top: gpui::px(crate::px_to_vp(self.top as f32, density)),
            right: gpui::px(crate::px_to_vp(self.right as f32, density)),
            bottom: gpui::px(crate::px_to_vp(self.bottom as f32, density)),
            left: gpui::px(crate::px_to_vp(self.left as f32, density)),
        }
    }
}

// ── OHOS 原生句柄 ────────────────────────────────────────────────────────────

#[cfg(target_env = "ohos")]
pub use ohos::{NativeWindow, RawOhosWindow, component_size_vp, native_window_from_surface_id};

#[cfg(target_env = "ohos")]
mod ohos {
    use std::ffi::c_void;
    use raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, OhosDisplayHandle, OhosNdkWindowHandle, RawDisplayHandle,
    };
    use std::ptr::NonNull;

    /// `OHNativeWindow*` 的薄封装（不拥有引用计数：句柄由 XComponent 持有，
    /// 只在 `created..destroyed` 区间有效，见风险 R10）。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct NativeWindow(NonNull<c_void>);

    // SAFETY: `OHNativeWindow` 是系统对象，读写由 EGL/窗口系统内部同步；
    // 本 crate 只在渲染线程上把它交给 wgpu。
    unsafe impl Send for NativeWindow {}
    unsafe impl Sync for NativeWindow {}

    impl NativeWindow {
        /// 从 XComponent 回调给出的 `void* window` 构造；空指针返回 `None`。
        pub fn new(pointer: *mut c_void) -> Option<Self> {
            NonNull::new(pointer).map(Self)
        }

        pub fn as_ptr(self) -> *mut c_void {
            self.0.as_ptr()
        }

        /// `GET_BUFFER_GEOMETRY`（头文件参数顺序是 `height, width`，单位 px）。
        pub fn buffer_geometry(self) -> Option<(u32, u32)> {
            let mut height: i32 = 0;
            let mut width: i32 = 0;
            // SAFETY: 句柄有效；可变参数按头文件声明的类型传入。
            let status = unsafe {
                OH_NativeWindow_NativeWindowHandleOpt(
                    self.0.as_ptr(),
                    GET_BUFFER_GEOMETRY,
                    &mut height as *mut i32,
                    &mut width as *mut i32,
                )
            };
            if status == 0 && height > 0 && width > 0 {
                Some((width as u32, height as u32))
            } else {
                None
            }
        }
    }

    /// 交给 `WgpuRenderer::new` 的、满足 `Clone + Debug + Send + Sync + 'static`
    /// 的窗口句柄包装。
    #[derive(Clone, Copy, Debug)]
    pub struct RawOhosWindow {
        native_window: NonNull<c_void>,
    }

    // SAFETY: 见 `NativeWindow`。
    unsafe impl Send for RawOhosWindow {}
    unsafe impl Sync for RawOhosWindow {}

    impl RawOhosWindow {
        pub fn new(native_window: NativeWindow) -> Self {
            Self {
                native_window: native_window.0,
            }
        }

        pub fn native_window(&self) -> NativeWindow {
            NativeWindow(self.native_window)
        }
    }

    impl HasWindowHandle for RawOhosWindow {
        fn window_handle(
            &self,
        ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            let handle = OhosNdkWindowHandle::new(self.native_window);
            // SAFETY: `OhosNdkWindowHandle` 不含借用，句柄的有效期由调用方保证。
            Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
        }
    }

    impl HasDisplayHandle for RawOhosWindow {
        fn display_handle(
            &self,
        ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
            // SAFETY: OHOS display handle 没有任何字段/生命周期。
            Ok(unsafe {
                raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Ohos(
                    OhosDisplayHandle::new(),
                ))
            })
        }
    }

    /// `OH_NativeWindow_NativeWindowHandleOpt` 的操作码（`NativeWindowOperation`）。
    const GET_BUFFER_GEOMETRY: i32 = 1;

    unsafe extern "C" {
        fn OH_NativeWindow_NativeWindowHandleOpt(
            window: *mut c_void,
            code: i32,
            ...
        ) -> i32;
        fn OH_NativeWindow_CreateNativeWindowFromSurfaceId(
            surface_id: u64,
            window: *mut *mut c_void,
        ) -> i32;
        fn OH_NativeXComponent_GetXComponentSize(
            component: *mut c_void,
            window: *const c_void,
            width: *mut u64,
            height: *mut u64,
        ) -> i32;
    }

    /// XComponent 的 surface 尺寸（单位 vp）。
    pub fn component_size_vp(component: *mut c_void, window: NativeWindow) -> Option<(f32, f32)> {
        if component.is_null() {
            return None;
        }
        let mut width: u64 = 0;
        let mut height: u64 = 0;
        // SAFETY: component 由模块初始化时保存，window 在 attach 区间内有效。
        let status = unsafe {
            OH_NativeXComponent_GetXComponentSize(
                component,
                window.as_ptr().cast_const(),
                &mut width,
                &mut height,
            )
        };
        if status == 0 && width > 0 && height > 0 {
            Some((width as f32, height as f32))
        } else {
            None
        }
    }

    /// ArkTS `getXComponentSurfaceId()` 给出的 id → `OHNativeWindow*`。
    ///
    /// 只有 `attach_surface` 收不到 XComponent 回调（例如宿主只传了 surface id）
    /// 时才走这条路。返回的句柄由调用方负责在 `detach_surface` 后不再使用。
    pub fn native_window_from_surface_id(surface_id: u64) -> Option<NativeWindow> {
        if surface_id == 0 {
            return None;
        }
        let mut pointer: *mut c_void = std::ptr::null_mut();
        // SAFETY: 出参指针有效；成功后返回系统持有的 OHNativeWindow。
        let status = unsafe { OH_NativeWindow_CreateNativeWindowFromSurfaceId(surface_id, &mut pointer) };
        if status == 0 {
            NativeWindow::new(pointer)
        } else {
            log::warn!("OH_NativeWindow_CreateNativeWindowFromSurfaceId({surface_id}) failed: {status}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_size_never_zero() {
        assert_eq!(sanitize_size(0, 0), (1, 1));
        assert_eq!(sanitize_size(1080, 1920), (1080, 1920));
    }

    #[test]
    fn surface_size_from_vp_rounds_and_clamps() {
        assert_eq!(surface_size_from_vp(360.0, 780.0, 3.0), (1080, 2340));
        assert_eq!(surface_size_from_vp(0.0, 0.0, 2.0), (1, 1));
    }

    #[test]
    fn insets_clamp_negative_and_take_max_per_edge() {
        let a = InsetsPx::new(10, -5, 20, 0);
        assert_eq!(a, InsetsPx { top: 10, right: 0, bottom: 20, left: 0 });
        let b = InsetsPx::new(0, 8, 40, 3);
        assert_eq!(a.max(b), InsetsPx { top: 10, right: 8, bottom: 40, left: 3 });
        assert!(InsetsPx::default().is_zero());
    }

    #[test]
    fn insets_convert_to_logical_edges() {
        let edges = InsetsPx::new(96, 0, 48, 0).to_edges(3.0);
        assert!((f32::from(edges.top) - 32.0).abs() < 1e-3);
        assert!((f32::from(edges.bottom) - 16.0).abs() < 1e-3);
        assert_eq!(f32::from(edges.left), 0.0);
    }
}
