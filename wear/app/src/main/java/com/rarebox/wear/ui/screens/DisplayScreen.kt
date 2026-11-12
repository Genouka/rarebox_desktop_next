package com.rarebox.wear.ui.screens

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.Text
import com.rarebox.wear.R
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.ui.InfoLine
import com.rarebox.wear.ui.IconGrid
import com.rarebox.wear.ui.Route
import com.rarebox.wear.ui.SimpleTextField
import com.rarebox.wear.ui.WearButton
import com.rarebox.wear.ui.WearCompact
import com.rarebox.wear.ui.SectionHeader
import com.rarebox.wear.ui.WearCard
import com.rarebox.wear.ui.WearScreen

/** 屏幕 Hub：截图/录屏与显示参数分开，避免一页堆满。 */
@Composable
fun DisplayScreen(
    repo: EngineRepository,
    state: CoreState,
    onNavigate: (Route) -> Unit,
    onBack: () -> Unit,
) {
    WearScreen(title = "屏幕", onBack = onBack) {
        item {
            IconGrid(
                entries = listOf(
                    Triple(R.drawable.ic_camera, "截图录屏", { onNavigate(Route.Screenshot) }),
                    Triple(R.drawable.ic_screen, "显示参数", { onNavigate(Route.DisplayParams) }),
                ),
            )
        }
        item {
            WearCard(title = "当前") {
                InfoLine("分辨率", state.screen.size)
                InfoLine("密度", state.screen.density)
                InfoLine("最近截图", state.lastScreenshot?.substringAfterLast('/'))
                InfoLine("最近录屏", state.lastRecording?.substringAfterLast('/'))
            }
        }
    }
}

/** 截图 / 录屏 / 分享。hdc 与 adb 分开实现，界面保持一致。 */
@Composable
fun ScreenshotScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
    onShare: (String) -> Unit,
) {
    WearScreen(title = "截图录屏", onBack = onBack) {
        if (state.harmony) {
            item {
                WearButton(icon = R.drawable.ic_camera, label = "截图", enabled = state.online) {
                    repo.send(Cmd.hdcScreenshot())
                }
            }
            item {
                WearButton(icon = R.drawable.ic_bolt, label = "唤醒屏幕") {
                    repo.send(Cmd.hdcWakeScreen())
                }
            }
        } else {
            item {
                WearButton(icon = R.drawable.ic_camera, label = "截图", enabled = state.online) {
                    repo.send(Cmd.takeScreenshot())
                }
            }
            item {
                WearButton(icon = R.drawable.ic_play, label = "录制 10 秒", enabled = state.online) {
                    repo.send(Cmd.recordScreen())
                }
            }
        }
        state.lastScreenshot?.let { path ->
            item {
                WearCard(title = "最近截图") {
                    Text(
                        path.substringAfterLast('/'),
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            item {
                WearButton(icon = R.drawable.ic_upload, label = "分享截图") { onShare(path) }
            }
        }
        if (!state.harmony) {
            state.lastRecording?.let { path ->
                item {
                    WearCard(title = "最近录屏") {
                        Text(
                            path.substringAfterLast('/'),
                            style = MaterialTheme.typography.bodySmall,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                item {
                    WearButton(icon = R.drawable.ic_upload, label = "分享录屏") { onShare(path) }
                }
            }
        }
    }
}

/** 显示参数：分辨率 / DPI / 旋转 / 输入文字。 */
@Composable
fun DisplayParamsScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
) {
    var text by remember { mutableStateOf("") }
    WearScreen(title = "显示参数", onBack = onBack) {
        item {
            WearCard(title = "分辨率") {
                InfoLine("当前", state.screen.size)
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_refresh, label = "重置分辨率", enabled = state.online) {
                repo.send(Cmd.resetDisplaySize())
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_screen, label = "540x540", enabled = state.online) {
                repo.send(Cmd.applyDisplaySize("540x540"))
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_screen, label = "1080x1080", enabled = state.online) {
                repo.send(Cmd.applyDisplaySize("1080x1080"))
            }
        }
        item {
            WearCard(title = "DPI") {
                InfoLine("当前", state.screen.density)
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_refresh, label = "重置 DPI", enabled = state.online) {
                repo.send(Cmd.resetDensity())
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_screen, label = "320 dpi", enabled = state.online) {
                repo.send(Cmd.applyDensity("320"))
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_screen, label = "420 dpi", enabled = state.online) {
                repo.send(Cmd.applyDensity("420"))
            }
        }
        item {
            WearCard(title = "旋转") { }
        }
        item {
            WearCompact(icon = R.drawable.ic_rotate, label = "0°", enabled = state.online) { repo.send(Cmd.setRotation(0)) }
        }
        item {
            WearCompact(icon = R.drawable.ic_rotate, label = "90°", enabled = state.online) { repo.send(Cmd.setRotation(1)) }
        }
        item {
            WearCompact(icon = R.drawable.ic_rotate, label = "180°", enabled = state.online) { repo.send(Cmd.setRotation(2)) }
        }
        item {
            WearCompact(icon = R.drawable.ic_rotate, label = "270°", enabled = state.online) { repo.send(Cmd.setRotation(3)) }
        }
        item {
            WearCard(title = "输入文字") {
                SimpleTextField(text, "要发送到设备的文字", { text = it })
            }
        }
        item {
            WearButton(
                icon = R.drawable.ic_keyboard,
                label = "发送",
                enabled = text.isNotBlank() && state.online,
            ) {
                repo.send(Cmd.sendText(text))
                text = ""
            }
        }
        item {
            WearCard {
                Text(
                    "重启与卸载等危险操作会先在手表上弹出确认框。",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
