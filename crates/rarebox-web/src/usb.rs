//! WebUSB 设备发现、接口认领与字节流实现。
//!
//! 浏览器只能通过 `navigator.usb` 访问 USB 设备，且必须先由用户手势触发
//! `requestDevice` 授权。ADB 设备的标准接口是 class `0xFF`、subclass `0x42`、
//! protocol `0x01`，两个 bulk 端点（IN/OUT）。
//!
//! 注意：Chrome 的 WebUSB 不支持在 Worker 里使用，也不支持与已占用接口的
//! 本机 adb server 共享设备——使用本页面前需要结束本机 adb 服务。

use anyhow::{Result, anyhow, bail};
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    UsbConfiguration, UsbDevice, UsbDeviceFilter, UsbDeviceRequestOptions, UsbDirection,
    UsbEndpoint, UsbEndpointType, UsbTransferStatus,
};

use crate::adb::Link;

const USB_CLASS_VENDOR: u8 = 0xff;
const ADB_SUBCLASS: u8 = 0x42;
const ADB_PROTOCOL: u8 = 0x01;
/// 单次 WebUSB 传输大小。ADB 消息本身只有 4 KiB payload，留足余量。
const TRANSFER_CHUNK: usize = 16 * 1024;

fn navigator() -> Result<web_sys::Navigator> {
    let window = web_sys::window().ok_or_else(|| anyhow!("当前环境没有 window 对象"))?;
    Ok(window.navigator())
}

/// 把浏览器异常转成可展示的中文错误。
pub fn js_error(value: JsValue) -> anyhow::Error {
    if let Some(message) = value.as_string() {
        return anyhow!(message);
    }
    if let Ok(message) = js_sys::Reflect::get(&value, &JsValue::from_str("message"))
        && let Some(message) = message.as_string()
    {
        return anyhow!(message);
    }
    anyhow!("浏览器拒绝了 USB 操作（{}）", format!("{value:?}"))
}

pub fn usb_available() -> bool {
    web_sys::window()
        .and_then(|window| {
            js_sys::Reflect::get(&window.navigator(), &JsValue::from_str("usb")).ok()
        })
        .map(|value| !value.is_undefined() && !value.is_null())
        .unwrap_or(false)
}

#[derive(Clone, serde::Serialize)]
pub struct AuthorizedDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_name: Option<String>,
    pub manufacturer_name: Option<String>,
    pub serial_number: Option<String>,
    pub has_adb_interface: bool,
}

impl AuthorizedDevice {
    fn describe(device: &UsbDevice) -> Self {
        Self {
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
            product_name: device.product_name(),
            manufacturer_name: device.manufacturer_name(),
            serial_number: device.serial_number(),
            has_adb_interface: find_adb_interface(device).is_some(),
        }
    }
}

/// 已授权给本页面的 USB 设备（不弹选择框）。
pub async fn authorized_devices() -> Result<Vec<(UsbDevice, AuthorizedDevice)>> {
    let devices = navigator()?.usb().get_devices().await.map_err(js_error)?;
    let mut result = Vec::new();
    for index in 0..devices.length() {
        let device: UsbDevice = devices
            .get(index)
            .dyn_into()
            .map_err(|_| anyhow!("无法读取 USB 设备"))?;
        let description = AuthorizedDevice::describe(&device);
        result.push((device, description));
    }
    Ok(result)
}

/// 弹出浏览器的 USB 选择框，返回用户选中的设备。
pub async fn request_device() -> Result<UsbDevice> {
    let mut filters = Vec::with_capacity(2);
    for protocol in [ADB_PROTOCOL, 0x00] {
        let filter = UsbDeviceFilter::default();
        UsbDeviceFilter::set_class_code(&filter, USB_CLASS_VENDOR);
        UsbDeviceFilter::set_subclass_code(&filter, ADB_SUBCLASS);
        UsbDeviceFilter::set_protocol_code(&filter, protocol);
        filters.push(filter);
    }
    let options = UsbDeviceRequestOptions::new(&filters);
    navigator()?
        .usb()
        .request_device(&options)
        .await
        .map_err(js_error)
}

#[derive(Clone, Copy, Debug)]
pub struct AdbInterface {
    pub configuration: u8,
    pub interface: u8,
    pub alternate: u8,
    pub endpoint_in: u8,
    pub endpoint_out: u8,
}

/// 找到 ADB 接口及其 bulk 端点。
pub fn find_adb_interface(device: &UsbDevice) -> Option<AdbInterface> {
    let current = device.configuration();
    let configurations = device.configurations();
    let mut candidates = Vec::new();
    if let Some(current) = current.clone() {
        candidates.push(current);
    }
    for index in 0..configurations.length() {
        if let Ok(configuration) = configurations.get(index).dyn_into::<UsbConfiguration>() {
            candidates.push(configuration);
        }
    }

    for configuration in candidates {
        let interfaces = configuration.interfaces();
        for index in 0..interfaces.length() {
            let Ok(interface) = interfaces.get(index).dyn_into::<web_sys::UsbInterface>() else {
                continue;
            };
            let alternates = interface.alternates();
            for alternate_index in 0..alternates.length() {
                let Ok(alternate) = alternates
                    .get(alternate_index)
                    .dyn_into::<web_sys::UsbAlternateInterface>()
                else {
                    continue;
                };
                if alternate.interface_class() != USB_CLASS_VENDOR
                    || alternate.interface_subclass() != ADB_SUBCLASS
                    || !matches!(alternate.interface_protocol(), ADB_PROTOCOL | 0x00)
                {
                    continue;
                }
                let endpoints = alternate.endpoints();
                let mut endpoint_in = None;
                let mut endpoint_out = None;
                for endpoint_index in 0..endpoints.length() {
                    let Ok(endpoint) = endpoints.get(endpoint_index).dyn_into::<UsbEndpoint>()
                    else {
                        continue;
                    };
                    if endpoint.type_() != UsbEndpointType::Bulk {
                        continue;
                    }
                    match endpoint.direction() {
                        UsbDirection::In => endpoint_in = Some(endpoint.endpoint_number()),
                        UsbDirection::Out => endpoint_out = Some(endpoint.endpoint_number()),
                        _ => {}
                    }
                }
                if let (Some(endpoint_in), Some(endpoint_out)) = (endpoint_in, endpoint_out) {
                    return Some(AdbInterface {
                        configuration: configuration.configuration_value(),
                        interface: interface.interface_number(),
                        alternate: alternate.alternate_setting(),
                        endpoint_in,
                        endpoint_out,
                    });
                }
            }
        }
    }
    None
}

/// 打开设备、认领 ADB 接口，并返回可读写的字节流。
pub struct UsbLink {
    device: UsbDevice,
    interface: AdbInterface,
}

impl UsbLink {
    pub async fn open(device: UsbDevice) -> Result<Self> {
        let interface = find_adb_interface(&device).ok_or_else(|| {
            anyhow!(
                "这台 USB 设备没有 ADB 接口。请打开「开发者选项 → USB 调试」，\
                 并在系统弹窗中允许 USB 访问。"
            )
        })?;

        if !device.opened() {
            device.open().await.map_err(|err| {
                anyhow!(
                    "无法打开 USB 设备：{}。若本机 adb server 正在运行，请先结束它（adb kill-server），\
                     因为 ADB 接口一次只能被一个程序占用。",
                    js_error(err)
                )
            })?;
        }

        let current_configuration = device
            .configuration()
            .map(|configuration| configuration.configuration_value());
        if current_configuration != Some(interface.configuration) {
            device
                .select_configuration(interface.configuration)
                .await
                .map_err(|err| anyhow!("无法切换 USB 配置：{}", js_error(err)))?;
        }

        device.claim_interface(interface.interface).await.map_err(|err| {
            let _ = device.close();
            anyhow!(
                "无法占用 ADB 接口：{}。请确认没有其它页面、adb server 或厂商工具正在使用该设备。",
                js_error(err)
            )
        })?;

        if interface.alternate != 0 {
            let _ = device
                .select_alternate_interface(interface.interface, interface.alternate)
                .await;
        }

        Ok(Self { device, interface })
    }

    pub async fn close(&mut self) {
        let _ = self
            .device
            .release_interface(self.interface.interface)
            .await;
        let _ = self.device.close().await;
    }
}

impl Link for UsbLink {
    async fn write_all(&mut self, data: &[u8]) -> Result<()> {
        for chunk in data.chunks(TRANSFER_CHUNK) {
            let promise = self
                .device
                .transfer_out_with_u8_slice(self.interface.endpoint_out, chunk)
                .map_err(js_error)?;
            let result = promise.await.map_err(js_error)?;
            match result.status() {
                UsbTransferStatus::Ok => {}
                status => bail!("USB 写入失败（{status:?}）"),
            }
            if result.bytes_written() as usize != chunk.len() {
                bail!(
                    "USB 写入不完整：{} / {} 字节",
                    result.bytes_written(),
                    chunk.len()
                );
            }
        }
        Ok(())
    }

    async fn read_some(&mut self, buffer: &mut [u8]) -> Result<usize> {
        let length = buffer.len().min(TRANSFER_CHUNK) as u32;
        loop {
            let result = self
                .device
                .transfer_in(self.interface.endpoint_in, length)
                .await
                .map_err(|err| anyhow!("USB 读取失败：{}", js_error(err)))?;
            match result.status() {
                UsbTransferStatus::Ok => {}
                UsbTransferStatus::Stall => {
                    // ADB 在设备断开时可能返回 STALL；清理一次后让上层重试。
                    let _ = self
                        .device
                        .clear_halt(UsbDirection::In, self.interface.endpoint_in)
                        .await;
                    bail!("USB 端点 STALL，设备可能已断开或未授权");
                }
                status => bail!("USB 读取失败（{status:?}）"),
            }
            // USB 用零长度包（ZLP）结束“长度正好是最大包长”的传输；
            // 它不是连接关闭，继续读下一条消息。
            let Some(data) = result.data() else {
                continue;
            };
            if data.byte_length() == 0 {
                continue;
            }
            let view = Uint8Array::new_with_byte_offset_and_length(
                &data.buffer(),
                data.byte_offset() as u32,
                data.byte_length() as u32,
            );
            let copied = (view.length() as usize).min(buffer.len());
            view.copy_to(&mut buffer[..copied]);
            return Ok(copied);
        }
    }
}
