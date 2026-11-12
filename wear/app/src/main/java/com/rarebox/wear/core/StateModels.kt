package com.rarebox.wear.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** 与 rarebox-ffi 的 `state_json`（camelCase）一一对应；未知字段忽略。 */
@Serializable
data class CoreState(
    val deviceKind: String = "android",
    val adb: ToolInfo = ToolInfo(),
    val hdc: ToolInfo = ToolInfo(),
    val devices: List<AdbDevice> = emptyList(),
    val selected: String? = null,
    val snapshot: DeviceSnapshot? = null,
    val harmonyDevices: List<HdcDevice> = emptyList(),
    val harmonySelected: String? = null,
    val harmonySnapshot: HarmonySnapshot? = null,
    val activity: String? = null,
    val packages: List<AppPackage> = emptyList(),
    val harmonyPackages: List<HarmonyPackage> = emptyList(),
    val files: FilesState = FilesState(),
    val harmonyFiles: FilesState = FilesState(),
    val logs: List<LogLine> = emptyList(),
    val harmonyLogs: List<LogLine> = emptyList(),
    val terminal: List<TerminalEntry> = emptyList(),
    val harmonyTerminal: List<TerminalEntry> = emptyList(),
    val screen: ScreenInfo = ScreenInfo(),
    val lastScreenshot: String? = null,
    val lastRecording: String? = null,
    val lastExport: String? = null,
    val wireless: WirelessInfo = WirelessInfo(),
    val install: InstallInfo = InstallInfo(),
    val remote: RemoteInfo = RemoteInfo(),
    val kindMode: String = "自动",
    val autoKind: Boolean = true,
    val mdns: List<MdnsService> = emptyList(),
    val scan: ScanState = ScanState(),
    val connectError: ConnectError? = null,
) {
    val harmony: Boolean get() = deviceKind == "harmony"

    /** 当前工作区是否有在线设备。 */
    val online: Boolean
        get() = if (harmony) harmonyDevices.any { it.online } else devices.any { it.online }

    val currentName: String?
        get() = if (harmony) {
            harmonySelected ?: harmonyDevices.firstOrNull { it.online }?.key
        } else {
            snapshot?.let { "${it.manufacturer} ${it.model}".trim() }
                ?: selected
                ?: devices.firstOrNull { it.online }?.let { it.model.ifBlank { it.serial } }
        }
}

@Serializable
data class ToolInfo(val path: String? = null, val version: String? = null, val error: String? = null)

@Serializable
data class AdbDevice(
    val serial: String = "",
    val state: String = "",
    val model: String = "",
    val connection: String = "",
    val online: Boolean = false,
)

@Serializable
data class HdcDevice(
    val key: String = "",
    val connection: String = "",
    val state: String = "",
    val online: Boolean = false,
    val host: String? = null,
)

@Serializable
data class DeviceSnapshot(
    val serial: String = "",
    val model: String = "",
    val manufacturer: String = "",
    val android: String = "",
    val sdk: String = "",
    val buildId: String = "",
    val characteristics: String = "",
    val patch: String = "",
    val abi: String = "",
    val ip: String? = null,
    val uptimeSeconds: Long? = null,
    val load: String? = null,
    val battery: Battery = Battery(),
    val memory: Memory = Memory(),
    val storage: Storage = Storage(),
    val display: Display = Display(),
    val support: DebugSupport? = null,
)

@Serializable
data class Battery(
    val level: Int? = null,
    val status: Int? = null,
    val temperature: Double? = null,
    val voltage: Long? = null,
    val powered: Boolean = false,
)

@Serializable
data class Memory(val totalKb: Long? = null, val availableKb: Long? = null)

/** 单个调试通道的支持结论。`supported == null` 表示未检测。 */
@Serializable
data class Capability(
    val supported: Boolean? = null,
    val enabled: Boolean = false,
    val hint: String = "",
)

/** 三种调试通道的能力矩阵。 */
@Serializable
data class DebugSupport(
    val wireless: Capability = Capability(),
    val wlan: Capability = Capability(),
    val usb: Capability = Capability(),
    val recommended: String? = null,
)

/** `adb mdns services` 发现的设备。 */
@Serializable
data class MdnsService(
    val instance: String = "",
    val kind: String = "",
    val address: String = "",
)

@Serializable
data class ScanHit(
    val address: String = "",
    val kind: String? = null,
    val state: String = "",
)

@Serializable
data class ScanState(
    val running: Boolean = false,
    val done: Int = 0,
    val total: Int = 0,
    val progress: Float = 0f,
    val scope: String = "",
    val deep: Boolean = false,
    val hits: List<ScanHit> = emptyList(),
)

@Serializable
data class ConnectError(
    val address: String = "",
    val kind: String = "",
    val detail: String = "",
)

@Serializable
data class Storage(val size: String? = null, val used: String? = null, val available: String? = null, val percent: Int? = null)

@Serializable
data class Display(
    val physicalSize: String? = null,
    val overrideSize: String? = null,
    val physicalDensity: String? = null,
    val overrideDensity: String? = null,
    val rotation: Int? = null,
)

@Serializable
data class HarmonySnapshot(
    val key: String = "",
    val model: String = "",
    val brand: String = "",
    val osVersion: String = "",
    val apiVersion: String = "",
    val abi: String = "",
    val udid: String? = null,
    val ip: String? = null,
    val battery: Int? = null,
    val batteryStatus: String? = null,
    val batteryTemperature: Double? = null,
    val memory: Memory = Memory(),
    val storageTotalKb: Long? = null,
    val storageUsedKb: Long? = null,
    val uptimeSeconds: Long? = null,
    val deviceName: String? = null,
    val screenState: String? = null,
    val screenResolution: List<Int>? = null,
    val screenRotation: Int? = null,
    val wifiSsid: String? = null,
)

@Serializable
data class AppPackage(
    val name: String = "",
    val label: String = "",
    val versionName: String? = null,
    val versionCode: Long? = null,
    @SerialName("system") val systemApp: Boolean = false,
)

@Serializable
data class HarmonyPackage(val name: String = "", val label: String = "")

@Serializable
data class FilesState(
    val path: String = "/sdcard",
    val selected: String? = null,
    val entries: List<FileEntry> = emptyList(),
)

@Serializable
data class FileEntry(
    val name: String = "",
    @SerialName("isDir") val dir: Boolean = false,
    @SerialName("isLink") val link: Boolean = false,
    val permissions: String = "",
    val size: Long? = null,
    val sizeLabel: String = "",
    val modified: String = "",
)

@Serializable
data class LogLine(
    val raw: String = "",
    val time: String = "",
    val level: String = "",
    val tag: String = "",
    val message: String = "",
)

@Serializable
data class TerminalEntry(
    val command: String = "",
    val output: String = "",
    val success: Boolean = false,
    val durationMs: Long? = null,
    val time: String = "",
)

@Serializable
data class ScreenInfo(val size: String? = null, val density: String? = null)

@Serializable
data class WirelessInfo(val address: String? = null, val pairAddress: String? = null, val pairCode: String? = null)

@Serializable
data class InstallInfo(
    val scope: String? = null,
    val query: String? = null,
    val batchReports: List<InstallReport>? = null,
    val planGroups: Int? = null,
    val planFailures: Int? = null,
)

@Serializable
data class InstallReport(val title: String = "", val detail: String = "", val success: Boolean = false)

@Serializable
data class RemoteInfo(
    val phase: String = "idle",
    val error: String? = null,
    val width: Int = 0,
    val height: Int = 0,
    val generation: Long = 0,
    val frameCount: Long = 0,
) {
    val running: Boolean get() = phase == "running"
}

internal val CoreJson = Json {
    ignoreUnknownKeys = true
    isLenient = true
    coerceInputValues = true
    explicitNulls = false
}
