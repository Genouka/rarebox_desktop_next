package com.rarebox.wear.ui.screens

import androidx.compose.runtime.Composable
import com.rarebox.wear.R
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.ui.IconAction
import com.rarebox.wear.ui.InfoLine
import com.rarebox.wear.ui.WearButton
import com.rarebox.wear.ui.WearCard
import com.rarebox.wear.ui.WearScreen

@Composable
fun OverviewScreen(repo: EngineRepository, state: CoreState, onBack: () -> Unit) {
    WearScreen(
        title = "概览",
        onBack = onBack,
        actions = {
            IconAction(icon = R.drawable.ic_refresh, label = "刷新") {
                repo.send(if (state.harmony) Cmd.loadHdcSnapshot() else Cmd.loadSnapshot())
            }
        },
    ) {
        if (state.harmony) {
            val snapshot = state.harmonySnapshot
            item {
                WearCard(title = "设备") {
                    InfoLine("型号", snapshot?.let { "${it.brand} ${it.model}".trim() })
                    InfoLine("系统", snapshot?.osVersion)
                    InfoLine("API", snapshot?.apiVersion)
                    InfoLine("ABI", snapshot?.abi)
                    InfoLine("IP", snapshot?.ip)
                    InfoLine("电量", snapshot?.battery?.let { "$it%" })
                    InfoLine("屏幕", snapshot?.screenResolution?.joinToString("x"))
                    InfoLine("Wi-Fi", snapshot?.wifiSsid)
                }
            }
            item {
                WearCard(title = "资源") {
                    val memory = snapshot?.memory
                    if (memory?.totalKb != null && memory.availableKb != null) {
                        InfoLine("内存", "${memory.availableKb / 1024} / ${memory.totalKb / 1024} MB")
                    }
                    if (snapshot?.storageTotalKb != null && snapshot.storageUsedKb != null) {
                        InfoLine("存储", "${snapshot.storageUsedKb / 1024} / ${snapshot.storageTotalKb / 1024} MB")
                    }
                    InfoLine("运行时间", snapshot?.uptimeSeconds?.let { seconds(it) })
                }
            }
            item {
                WearButton(icon = R.drawable.ic_power, label = "重启 (System)", enabled = state.online) {
                    repo.send(Cmd.hdcReboot("System"))
                }
            }
        } else {
            val snapshot = state.snapshot
            item {
                WearCard(title = "设备") {
                    InfoLine("型号", snapshot?.let { "${it.manufacturer} ${it.model}".trim() })
                    InfoLine("Android", snapshot?.android)
                    InfoLine("SDK", snapshot?.sdk)
                    InfoLine("ABI", snapshot?.abi)
                    InfoLine("IP", snapshot?.ip)
                    InfoLine("电量", snapshot?.battery?.level?.let { "$it%" })
                    InfoLine("温度", snapshot?.battery?.temperature?.let { "%.1f°C".format(it) })
                    InfoLine("屏幕", snapshot?.display?.physicalSize)
                    InfoLine("密度", snapshot?.display?.physicalDensity)
                }
            }
            item {
                WearCard(title = "资源") {
                    val memory = snapshot?.memory
                    if (memory?.totalKb != null && memory.availableKb != null) {
                        InfoLine("内存", "${memory.availableKb / 1024} / ${memory.totalKb / 1024} MB")
                    }
                    val storage = snapshot?.storage
                    if (storage?.size != null) {
                        InfoLine("存储", "${storage.used} / ${storage.size}")
                    }
                    InfoLine("运行时间", snapshot?.uptimeSeconds?.let { seconds(it) })
                }
            }
        }
    }
}

private fun seconds(total: Long): String {
    val hours = total / 3600
    val minutes = (total % 3600) / 60
    return if (hours > 0) "${hours}h ${minutes}m" else "${minutes}m"
}
