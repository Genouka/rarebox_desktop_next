package com.rarebox.wear.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.wear.compose.foundation.lazy.TransformingLazyColumnItemScope
import androidx.wear.compose.material3.Button
import androidx.wear.compose.material3.ButtonDefaults
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.Text
import com.rarebox.wear.R
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.ui.IconAction
import com.rarebox.wear.ui.IconGrid
import com.rarebox.wear.ui.IconRow
import com.rarebox.wear.ui.InfoLine
import com.rarebox.wear.ui.Route
import com.rarebox.wear.ui.SectionHeader
import com.rarebox.wear.ui.SimpleTextField
import com.rarebox.wear.ui.WearButton
import com.rarebox.wear.ui.WearCard
import com.rarebox.wear.ui.WearCompact
import com.rarebox.wear.ui.WearRow
import com.rarebox.wear.ui.WearScreen

/** 连接 Hub：只放入口，具体流程都在子页。 */
@Composable
fun ConnectHubScreen(
    repo: EngineRepository,
    state: CoreState,
    onNavigate: (Route) -> Unit,
    onBack: () -> Unit,
) {
    LaunchedEffect(Unit) {
        // 一次并行探测：自动识别 Android / HarmonyOS，并刷新 mDNS。
        repo.send(Cmd.refreshAll())
    }
    val support = state.snapshot?.support
    WearScreen(
        title = "连接",
        onBack = onBack,
        actions = {
            IconAction(icon = R.drawable.ic_refresh, label = "重新识别") {
                repo.send(Cmd.refreshAll())
            }
        },
    ) {
        item { DeviceCard(state = state, onClick = null) }
        item {
            IconGrid(
                entries = listOf(
                    Triple(R.drawable.ic_wifi, "无线调试", { onNavigate(Route.ConnectWireless) }),
                    Triple(R.drawable.ic_key, "配对码", { onNavigate(Route.ConnectPair) }),
                    Triple(R.drawable.ic_router, "WLAN", { onNavigate(Route.ConnectWlan) }),
                    Triple(R.drawable.ic_usb, "USB", { onNavigate(Route.ConnectUsb) }),
                ),
            )
        }
        item {
            IconRow(
                entries = listOf(
                    Triple(R.drawable.ic_scan, "扫描", { onNavigate(Route.ConnectScan) }),
                    Triple(R.drawable.ic_devices, "设备", { onNavigate(Route.ConnectDevices) }),
                ),
            )
        }
        item { SectionHeader("手表能力") }
        item {
            WearCard {
                InfoLine("无线调试", capabilityLabel(support?.wireless))
                InfoLine("WLAN 调试", capabilityLabel(support?.wlan))
                InfoLine("USB 调试", capabilityLabel(support?.usb))
                support?.recommended?.let {
                    Text(
                        "推荐：$it",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.tertiary,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }
            }
        }
    }
}

private fun capabilityLabel(capability: com.rarebox.wear.core.Capability?): String = when {
    capability == null -> "未检测"
    capability.enabled -> "已开启"
    capability.supported == false -> "不支持 · ${capability.hint}"
    capability.supported == true -> "支持，未开启"
    else -> "未检测"
}

/** 无线调试：状态、配对入口、连接与断开。 */
@Composable
fun ConnectWirelessScreen(
    repo: EngineRepository,
    state: CoreState,
    onNavigate: (Route) -> Unit,
    onBack: () -> Unit,
) {
    val support = state.snapshot?.support?.wireless
    val connectService = state.mdns.firstOrNull { it.kind == "连接" }
    val address = connectService?.address ?: state.snapshot?.ip?.let { "$it:5555" }
    WearScreen(title = "无线调试", onBack = onBack) {
        item { SectionHeader("状态", capabilityLabel(support)) }
        item {
            WearCard {
                InfoLine("设备", state.currentName ?: "未连接")
                InfoLine("在线", if (state.online) "是" else "否")
                connectService?.let { InfoLine("已发现", it.address) }
            }
        }
        item { SectionHeader("操作") }
        item {
            WearButton(
                icon = R.drawable.ic_link,
                label = "连接",
                enabled = address != null,
            ) {
                if (address != null) repo.send(Cmd.connectSmart(address))
            }
        }
        item {
            WearCompact(
                icon = R.drawable.ic_key,
                label = "用配对码配对",
            ) { onNavigate(Route.ConnectPair) }
        }
        item {
            WearCompact(
                icon = R.drawable.ic_unlink,
                label = "断开无线连接",
                enabled = state.online || connectService != null,
            ) { repo.send(Cmd.wirelessDisconnect()) }
        }
        if (address == null) {
            item {
                WearCard {
                    Text(
                        "还没有可用的连接地址。先在手表上打开「无线调试」，或去「扫描」找设备。",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

/** 配对码：地址可自动填，六位码用自带键盘。 */
@Composable
fun ConnectPairScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
) {
    val pairing = state.mdns.firstOrNull { it.kind == "配对" }
    var address by remember { mutableStateOf(pairing?.address ?: "") }
    var code by remember { mutableStateOf("") }
    LaunchedEffect(pairing?.address) {
        if (pairing != null && address.isBlank()) address = pairing.address
    }
    val ready = code.length == 6 && address.isNotBlank()
    WearScreen(title = "配对码", onBack = onBack) {
        item { SectionHeader("配对地址") }
        item {
            WearCard {
                SimpleTextField(address, "IP:端口", { address = it })
                if (pairing != null) {
                    Text(
                        "已自动填入附近设备",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.tertiary,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }
            }
        }
        item { SectionHeader("六位配对码") }
        item {
            WearCard {
                Text(
                    text = code.padEnd(6, '·').chunked(1).joinToString(" "),
                    style = MaterialTheme.typography.titleMedium,
                    textAlign = TextAlign.Center,
                    maxLines = 1,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
        item {
            Keypad(
                onDigit = { digit -> if (code.length < 6) code += digit },
                onBackspace = { code = code.dropLast(1) },
            )
        }
        item {
            WearButton(
                icon = R.drawable.ic_check,
                label = "配对",
                enabled = ready,
            ) {
                repo.send(Cmd.wirelessPair(address.trim(), code))
            }
        }
        if (!ready) {
            item {
                Text(
                    "填好地址并输入六位码后可配对",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

/** 0-9 数字键盘 + 退格；避免在手表上拉起系统键盘。 */
@Composable
fun TransformingLazyColumnItemScope.Keypad(
    onDigit: (Char) -> Unit,
    onBackspace: () -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        listOf("123", "456", "789").forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                row.forEach { digit ->
                    KeypadKey(digit.toString(), Modifier.weight(1f)) { onDigit(digit) }
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            KeypadKey("⌫", Modifier.weight(1f)) { onBackspace() }
            KeypadKey("0", Modifier.weight(1f)) { onDigit('0') }
            KeypadKey("", Modifier.weight(1f), enabled = false) {}
        }
    }
}

@Composable
private fun KeypadKey(
    label: String,
    modifier: Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier,
        contentPadding = ButtonDefaults.ContentPadding,
    ) {
        Text(label, style = MaterialTheme.typography.titleMedium, maxLines = 1)
    }
}

/** WLAN 调试（旧版 5555）：先 USB 开启端口，再无线连。 */
@Composable
fun ConnectWlanScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
) {
    val ip = state.snapshot?.ip
    val support = state.snapshot?.support?.wlan
    WearScreen(title = "WLAN 调试", onBack = onBack) {
        item { SectionHeader("状态", capabilityLabel(support)) }
        item {
            WearCard {
                InfoLine("地址", ip?.let { "$it:5555" })
                InfoLine("设备", state.currentName ?: "未连接")
            }
        }
        item { SectionHeader("操作") }
        item {
            WearButton(
                icon = R.drawable.ic_bolt,
                label = "开启 TCP/IP 5555",
                enabled = state.online,
            ) { repo.send(Cmd.enableTcpip()) }
        }
        item {
            WearButton(
                icon = R.drawable.ic_link,
                label = "无线连接",
                enabled = ip != null,
            ) {
                if (ip != null) repo.send(Cmd.connectSmart("$ip:5555"))
            }
        }
        item {
            WearCompact(
                icon = R.drawable.ic_unlink,
                label = "断开无线连接",
                enabled = state.online,
            ) { repo.send(Cmd.wirelessDisconnect()) }
        }
        item {
            WearCard {
                Text(
                    "旧机型没有「配对码」时用这条路径：USB 授权一次，开启 5555 后拔线。",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** USB：三条图文步骤。 */
@Composable
fun ConnectUsbScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
) {
    WearScreen(title = "USB 调试", onBack = onBack) {
        item { SectionHeader("步骤") }
        item {
            WearCard {
                StepRow(1, "打开「USB 调试」")
                StepRow(2, "用数据线连接电脑")
                StepRow(3, "手表弹窗点「允许」")
            }
        }
        item { SectionHeader("状态", capabilityLabel(state.snapshot?.support?.usb)) }
        item {
            WearCard {
                InfoLine("设备", state.currentName ?: "未连接")
                InfoLine("在线", if (state.online) "是" else "否")
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_refresh, label = "重新检测") {
                repo.send(Cmd.refreshAll())
            }
        }
    }
}

@Composable
private fun StepRow(index: Int, text: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            "$index",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(text, style = MaterialTheme.typography.bodySmall)
    }
}

/** 盲扫：输入可留空，扫本机网段。 */
@Composable
fun ConnectScanScreen(
    repo: EngineRepository,
    state: CoreState,
    onNavigate: (Route) -> Unit,
    onBack: () -> Unit,
) {
    var address by remember { mutableStateOf("") }
    val scan = state.scan
    WearScreen(title = "扫描", onBack = onBack) {
        item { SectionHeader("范围") }
        item {
            WearCard {
                SimpleTextField(address, "留空扫本机网段，或填 IP", { address = it })
                InfoLine("范围", scan.scope.ifBlank { "—" })
                InfoLine("进度", "${scan.done}/${scan.total}")
            }
        }
        item {
            WearButton(
                icon = R.drawable.ic_scan,
                label = if (scan.running) "扫描中…" else "开始扫描",
                enabled = !scan.running,
            ) { repo.send(Cmd.scanDevices(address.trim(), false)) }
        }
        if (scan.running) {
            item {
                WearCompact(icon = R.drawable.ic_close, label = "取消扫描") {
                    repo.send(Cmd.cancelScan())
                }
            }
        }
        if (scan.hits.isNotEmpty()) {
            item { SectionHeader("结果", "${scan.hits.size} 个") }
            scan.hits.forEach { hit ->
                item {
                    WearRow(
                        icon = when (hit.kind) {
                            "android" -> R.drawable.ic_android
                            "harmony" -> R.drawable.ic_devices
                            else -> R.drawable.ic_scan
                        },
                        label = hit.address,
                        value = hit.state,
                        enabled = hit.kind != null,
                        onClick = if (hit.kind != null) {
                            {
                                repo.send(Cmd.connectSmart(hit.address))
                                onNavigate(Route.ConnectWireless)
                            }
                        } else {
                            null
                        },
                    )
                }
            }
        }
        if (!scan.running && scan.hits.isEmpty()) {
            item {
                WearCard {
                    Text(
                        "没扫到？可点「深度扫描」扫单机全部端口。",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            item {
                WearCompact(icon = R.drawable.ic_search, label = "深度扫描") {
                    repo.send(Cmd.scanDevices(address.trim(), true))
                }
            }
        }
    }
}

/** 设备列表：ADB / hdc 混合展示。 */
@Composable
fun ConnectDevicesScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
) {
    val empty = if (state.harmony) state.harmonyDevices.isEmpty() else state.devices.isEmpty()
    WearScreen(title = "设备", onBack = onBack) {
        item {
            WearCompact(icon = R.drawable.ic_refresh, label = "刷新设备") {
                repo.send(Cmd.refreshAll())
            }
        }
        if (state.harmony) {
            state.harmonyDevices.forEach { device ->
                item {
                    WearRow(
                        icon = R.drawable.ic_devices,
                        label = device.key,
                        value = device.state,
                        enabled = device.online,
                        onClick = { repo.send(Cmd.selectHdcDevice(device.key)) },
                    )
                }
            }
        } else {
            state.devices.forEach { device ->
                item {
                    WearRow(
                        icon = R.drawable.ic_android,
                        label = device.model.ifBlank { device.serial },
                        value = device.connection,
                        enabled = device.online,
                        onClick = { repo.send(Cmd.selectDevice(device.serial)) },
                    )
                }
            }
        }
        if (empty) {
            item {
                WearCard {
                    Text(
                        "未发现设备。用「扫描」或在手表上开启调试。",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
