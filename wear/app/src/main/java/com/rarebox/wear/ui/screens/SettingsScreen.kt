package com.rarebox.wear.ui.screens

import androidx.compose.runtime.Composable
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.Text
import com.rarebox.wear.BuildConfig
import com.rarebox.wear.R
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.ui.InfoLine
import com.rarebox.wear.ui.IconGrid
import com.rarebox.wear.ui.Route
import com.rarebox.wear.ui.WearButton
import com.rarebox.wear.ui.WearCompact
import com.rarebox.wear.ui.WearCard
import com.rarebox.wear.ui.WearScreen

/** 设置 Hub：三个子页，避免一页堆满。 */
@Composable
fun SettingsScreen(
    repo: EngineRepository,
    state: CoreState,
    onNavigate: (Route) -> Unit,
    onBack: () -> Unit,
) {
    WearScreen(title = "设置", onBack = onBack) {
        item {
            IconGrid(
                entries = listOf(
                    Triple(R.drawable.ic_info, "关于", { onNavigate(Route.SettingsAbout) }),
                    Triple(R.drawable.ic_bolt, "工具进程", { onNavigate(Route.SettingsTools) }),
                    Triple(R.drawable.ic_logs, "条款", { onNavigate(Route.SettingsLegal) }),
                ),
            )
        }
        item {
            WearCard(title = "当前设备") {
                InfoLine("设备", state.currentName ?: "未连接")
                InfoLine("协议", if (state.harmony) "HarmonyOS（hdc）" else "Android（ADB）")
            }
        }
    }
}

@Composable
fun SettingsAboutScreen(state: CoreState, onBack: () -> Unit) {
    WearScreen(title = "关于", onBack = onBack) {
        item {
            WearCard(title = "RareBox Wear") {
                InfoLine("版本", BuildConfig.VERSION_NAME)
                InfoLine("内核", "rarebox-core 0.2.0")
                InfoLine("投屏画质", "512px / 2 Mbps")
            }
        }
        item {
            WearCard(title = "设备") {
                InfoLine("设备", state.currentName ?: "未连接")
                InfoLine("adb", state.adb.version)
                InfoLine("hdc", state.hdc.version)
            }
        }
    }
}

@Composable
fun SettingsToolsScreen(repo: EngineRepository, state: CoreState, onBack: () -> Unit) {
    WearScreen(title = "工具进程", onBack = onBack) {
        item {
            WearCard(title = "版本") {
                InfoLine("adb", state.adb.version ?: state.adb.error)
                InfoLine("hdc", state.hdc.version ?: state.hdc.error)
            }
        }
        item {
            WearButton(icon = R.drawable.ic_stop, label = "结束 adb 进程", enabled = state.adb.version != null) {
                repo.send(Cmd.killAdbProcess())
            }
        }
        item {
            WearButton(icon = R.drawable.ic_stop, label = "结束 hdc 进程", enabled = state.hdc.version != null) {
                repo.send(Cmd.killHdcProcess())
            }
        }
    }
}

@Composable
fun SettingsLegalScreen(onBack: () -> Unit, onOpenUrl: (String) -> Unit) {
    WearScreen(title = "条款", onBack = onBack) {
        item {
            WearCard {
                Text(
                    "使用本应用即表示同意 RareBox 的用户协议与隐私政策；所有设备操作都在本机完成，不上传设备数据。",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_link, label = "查看项目主页") {
                onOpenUrl("https://rare.genouka.top")
            }
        }
        item {
            WearCard {
                Text(
                    "自调试依赖手表自身的「无线调试」；投屏为软件解码，长时间使用会增加耗电。",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
