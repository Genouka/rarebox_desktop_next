package com.rarebox.wear.core

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** 把命令编码为引擎要求的 JSON（字段与 `rarebox_core::command::Command` 一致）。 */
object Cmd {
    private fun simple(name: String) = """{"cmd":"$name"}"""

    private fun objectCmd(name: String, vararg fields: Pair<String, JsonElement>) =
        buildJsonObject {
            put("cmd", name)
            fields.forEach { (key, value) -> put(key, value) }
        }.toString()

    private fun stringCmd(name: String, vararg fields: Pair<String, String>) =
        objectCmd(name, *fields.map { (key, value) -> key to JsonPrimitive(value) }.toTypedArray())

    fun refreshDevices() = simple("refresh_devices")
    fun refreshHdc() = simple("refresh_hdc")
    fun loadSnapshot() = simple("load_snapshot")
    fun loadHdcSnapshot() = simple("load_hdc_snapshot")

    fun selectDevice(serial: String) = stringCmd("select_device", "serial" to serial)
    fun selectHdcDevice(key: String) = stringCmd("select_hdc_device", "key" to key)
    fun setDeviceKind(kind: String) = stringCmd("set_device_kind", "kind" to kind)

    /** 自动识别 / 手动锁定：`auto` / `android` / `harmony`。 */
    fun setKindMode(mode: String) = stringCmd("set_kind_mode", "mode" to mode)

    // 同时刷新 ADB、hdc 与 mDNS。
    fun refreshAll() = simple("refresh_all")

    // 一个框连接：地址 + 可选配对码，由内核区分 ADB / hdc。
    fun connectSmart(address: String, code: String? = null) =
        if (code.isNullOrBlank()) {
            objectCmd("connect_smart", "address" to JsonPrimitive(address))
        } else {
            objectCmd(
                "connect_smart",
                "address" to JsonPrimitive(address),
                "code" to JsonPrimitive(code),
            )
        }

    // 盲扫局域网里的调试端口。
    fun scanDevices(address: String, deep: Boolean) = objectCmd(
        "scan_devices",
        "address" to JsonPrimitive(address),
        "deep" to JsonPrimitive(deep),
    )

    fun cancelScan() = simple("cancel_scan")

    fun wirelessConnect(address: String) = stringCmd("wireless_connect", "address" to address)
    fun wirelessPair(address: String, code: String) =
        stringCmd("wireless_pair", "address" to address, "code" to code)
    fun wirelessDisconnect() = simple("wireless_disconnect")
    fun enableTcpip() = simple("enable_tcpip")
    fun disconnectDevice(serial: String) = stringCmd("disconnect_device", "serial" to serial)

    fun loadPackages() = simple("load_packages")
    fun pickInstallApks() = simple("pick_install_apks")
    fun pickInstallHaps() = simple("pick_install_haps")
    fun pickPushFiles() = simple("pick_push_files")
    fun pickPushHdcFiles() = simple("pick_push_hdc_files")
    fun installApks(paths: List<String>) = objectCmd("install_apks", "paths" to pathsJson(paths))
    fun launchPackage(name: String) = stringCmd("launch_package", "package" to name)
    fun stopPackage(name: String) = stringCmd("stop_package", "package" to name)
    fun clearPackage(name: String) = stringCmd("clear_package", "package" to name)
    fun uninstallPackage(name: String) = stringCmd("uninstall_package", "package" to name)

    fun listFiles() = simple("list_files")
    fun navigateFiles(path: String) = stringCmd("navigate_files", "path" to path)
    fun goParentFiles() = simple("go_parent_files")
    fun pullFile(remote: String) = stringCmd("pull_file", "remote" to remote)
    fun pushFiles(paths: List<String>) = objectCmd("push_files", "paths" to pathsJson(paths))

    fun loadLogs() = simple("load_logs")
    fun clearLogcat() = simple("clear_logcat")

    fun takeScreenshot() = simple("take_screenshot")
    fun recordScreen() = simple("record_screen")
    fun applyDisplaySize(size: String) = stringCmd("apply_display_size", "size" to size)
    fun resetDisplaySize() = simple("reset_display_size")
    fun applyDensity(density: String) = stringCmd("apply_density", "density" to density)
    fun resetDensity() = simple("reset_density")
    fun setRotation(rotation: Int) = objectCmd("set_rotation", "rotation" to JsonPrimitive(rotation))
    fun sendKey(code: Int, label: String) = objectCmd("send_key", "code" to JsonPrimitive(code), "label" to JsonPrimitive(label))
    fun sendText(text: String) = stringCmd("send_text", "text" to text)

    fun submitTerminal(command: String) = stringCmd("submit_terminal", "command" to command)
    fun runQuickCommand(command: String) = stringCmd("run_quick_command", "command" to command)

    fun startRemote() = simple("start_remote")
    fun stopRemote() = simple("stop_remote")
    fun remotePointerDown(x: Int, y: Int) = objectCmd("remote_pointer_down", "x" to JsonPrimitive(x), "y" to JsonPrimitive(y))
    fun remotePointerMove(x: Int, y: Int) = objectCmd("remote_pointer_move", "x" to JsonPrimitive(x), "y" to JsonPrimitive(y))
    fun remotePointerUp(x: Int, y: Int) = objectCmd("remote_pointer_up", "x" to JsonPrimitive(x), "y" to JsonPrimitive(y))
    fun sendRemoteKey(keycode: Int) = objectCmd("send_remote_key", "keycode" to JsonPrimitive(keycode))
    fun sendRemoteText(text: String) = stringCmd("send_remote_text", "text" to text)

    fun hdcConnect(address: String) = stringCmd("hdc_connect", "address" to address)
    fun hdcDisconnect() = simple("hdc_disconnect")
    fun hdcLoadPackages() = simple("hdc_load_packages")
    fun hdcInstallHaps(paths: List<String>) = objectCmd("hdc_install_haps", "paths" to pathsJson(paths))
    fun hdcLaunchPackage(bundle: String) = stringCmd("hdc_launch_package", "bundle" to bundle)
    fun hdcStopPackage(bundle: String) = stringCmd("hdc_stop_package", "bundle" to bundle)
    fun hdcUninstallPackage(bundle: String) = stringCmd("hdc_uninstall_package", "bundle" to bundle)
    fun hdcClearAppData(bundle: String, cache: Boolean) =
        objectCmd("hdc_clear_app_data", "bundle" to JsonPrimitive(bundle), "cache" to JsonPrimitive(cache))
    fun hdcLoadFiles() = simple("hdc_load_files")
    fun hdcNavigateFiles(path: String) = stringCmd("hdc_navigate_files", "path" to path)
    fun hdcGoParentFiles() = simple("hdc_go_parent_files")
    fun hdcPullFile(remote: String) = stringCmd("hdc_pull_file", "remote" to remote)
    fun hdcUploadFiles(paths: List<String>) = objectCmd("hdc_upload_files", "paths" to pathsJson(paths))
    fun hdcLoadLogs(tag: String = "") = stringCmd("hdc_load_logs", "tag" to tag, "filter" to "all")
    fun hdcScreenshot() = simple("hdc_screenshot")
    fun hdcWakeScreen() = simple("hdc_wake_screen")
    fun hdcReboot(target: String) = stringCmd("hdc_reboot", "target" to target)
    fun hdcSubmitTerminal(command: String) = stringCmd("hdc_submit_terminal", "command" to command)

    fun setScreenshotDir(path: String) = stringCmd("set_screenshot_dir", "path" to path)

    fun acceptLegal() = simple("accept_legal")
    fun killAdbProcess() = simple("kill_adb_process")
    fun killHdcProcess() = simple("kill_hdc_process")

    fun executeConfirm(kind: JsonElement) = objectCmd("execute_confirm", "kind" to kind)
    fun requestReboot(target: String) = stringCmd("request_reboot", "target" to target)

    private fun pathsJson(paths: List<String>) = buildJsonArray {
        paths.forEach { add(JsonPrimitive(it)) }
    }
}
