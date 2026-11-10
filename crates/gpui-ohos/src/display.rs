//! 鸿蒙显示信息。
//!
//! OHOS 的 native 侧没有“显示器枚举”API：一个 ability 只有一个主窗口，尺寸来自
//! XComponent surface 或 ArkTS 的 `windowSizeChange`，密度来自
//! `display.getDefaultDisplaySync().densityPixels`（由 NAPI `start` 传入）。
//! 因此 [`OhosDisplay`] 是一个由平台状态驱动、可更新的单显示器模型。

use anyhow::Result;
use gpui::{Bounds, DisplayId, PlatformDisplay, Pixels, point, px, size};
use std::fmt;
use uuid::Uuid;

/// 一个逻辑显示器。
#[derive(Clone, Debug)]
pub struct OhosDisplay {
    width_px: u32,
    height_px: u32,
    scale_factor: f32,
    id: u64,
}

impl OhosDisplay {
    /// 用物理像素尺寸与密度构造。
    pub fn new(width_px: u32, height_px: u32, scale_factor: f32, id: u64) -> Self {
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        Self {
            width_px: width_px.max(1),
            height_px: height_px.max(1),
            scale_factor,
            id,
        }
    }

    /// 尚未拿到 surface 尺寸时的占位显示器。
    pub fn headless(width_px: u32, height_px: u32) -> Self {
        Self::new(width_px, height_px, 1.0, 0)
    }

    pub fn physical_width(&self) -> u32 {
        self.width_px
    }

    pub fn physical_height(&self) -> u32 {
        self.height_px
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// 更新尺寸/密度，返回是否发生变化。
    pub fn resize(&mut self, width_px: u32, height_px: u32, scale_factor: f32) -> bool {
        let width_px = width_px.max(1);
        let height_px = height_px.max(1);
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let changed = self.width_px != width_px
            || self.height_px != height_px
            || (self.scale_factor - scale_factor).abs() > f32::EPSILON;
        self.width_px = width_px;
        self.height_px = height_px;
        self.scale_factor = scale_factor;
        changed
    }

    /// 逻辑尺寸（vp）。
    pub fn logical_size(&self) -> gpui::Size<Pixels> {
        crate::logical_size_from_device(self.width_px, self.height_px, self.scale_factor)
    }
}

impl PlatformDisplay for OhosDisplay {
    fn id(&self) -> DisplayId {
        DisplayId::new(self.id)
    }

    fn uuid(&self) -> Result<Uuid> {
        // OHOS 不给显示器稳定 UUID；用 id 派生一个进程内稳定的值。
        Ok(Uuid::from_u128(
            ((self.id as u128) << 64) | (self.scale_factor.to_bits() as u128),
        ))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let logical = self.logical_size();
        Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(logical.width, logical.height),
        }
    }
}

impl fmt::Display for OhosDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let logical = self.logical_size();
        write!(
            f,
            "OhosDisplay({}×{}px, {:.2}×, {:.0}×{:.0}vp)",
            self.width_px,
            self.height_px,
            self.scale_factor,
            f32::from(logical.width),
            f32::from(logical.height),
        )
    }
}

/// 当前连接的显示器列表（鸿蒙上恒为一个）。
#[derive(Clone, Debug)]
pub struct DisplayList {
    displays: Vec<OhosDisplay>,
}

impl DisplayList {
    pub fn single(display: OhosDisplay) -> Self {
        Self {
            displays: vec![display],
        }
    }

    pub fn primary(&self) -> Option<&OhosDisplay> {
        self.displays.first()
    }

    pub fn primary_mut(&mut self) -> Option<&mut OhosDisplay> {
        self.displays.first_mut()
    }

    pub fn all(&self) -> &[OhosDisplay] {
        &self.displays
    }

    /// 更新主显示器；没有主显示器时补一个。
    pub fn update_primary(&mut self, width_px: u32, height_px: u32, scale_factor: f32) -> bool {
        match self.primary_mut() {
            Some(display) => display.resize(width_px, height_px, scale_factor),
            None => {
                self.displays
                    .push(OhosDisplay::new(width_px, height_px, scale_factor, 1));
                true
            }
        }
    }
}

impl Default for DisplayList {
    fn default() -> Self {
        Self::single(OhosDisplay::headless(1, 1))
    }
}
