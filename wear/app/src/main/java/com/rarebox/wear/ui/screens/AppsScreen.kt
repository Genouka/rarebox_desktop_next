package com.rarebox.wear.ui.screens

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.text.style.TextOverflow
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.Text
import com.rarebox.wear.R
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.ui.WearButton
import com.rarebox.wear.ui.WearCompact
import com.rarebox.wear.ui.WearCard
import com.rarebox.wear.ui.WearRow
import com.rarebox.wear.ui.WearScreen

@Composable
fun AppsScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
    onPickInstall: () -> Unit,
    onOpenApp: (String, String) -> Unit,
) {
    LaunchedEffect(Unit) {
        repo.send(if (state.harmony) Cmd.hdcLoadPackages() else Cmd.loadPackages())
    }

    WearScreen(title = "应用", onBack = onBack) {
        item {
            WearCompact(icon = R.drawable.ic_refresh, label = "刷新列表", enabled = state.online) {
                repo.send(if (state.harmony) Cmd.hdcLoadPackages() else Cmd.loadPackages())
            }
        }
        item {
            WearButton(
                icon = R.drawable.ic_download,
                label = if (state.harmony) "安装 HAP" else "安装 APK",
                enabled = state.online,
            ) { onPickInstall() }
        }

        if (state.harmony) {
            if (state.harmonyPackages.isEmpty()) {
                item { EmptyApps() }
            }
            state.harmonyPackages.take(200).forEach { pkg ->
                item {
                    WearRow(
                        icon = R.drawable.ic_package,
                        label = pkg.label.ifBlank { pkg.name },
                        value = pkg.name,
                        onClick = { onOpenApp(pkg.name, pkg.label.ifBlank { pkg.name }) },
                    )
                }
            }
        } else {
            if (state.packages.isEmpty()) {
                item { EmptyApps() }
            }
            state.packages.take(200).forEach { pkg ->
                item {
                    WearRow(
                        icon = R.drawable.ic_package,
                        label = pkg.label.ifBlank { pkg.name },
                        value = pkg.versionName ?: "",
                        onClick = { onOpenApp(pkg.name, pkg.label.ifBlank { pkg.name }) },
                    )
                }
            }
        }
    }
}

@Composable
private fun androidx.wear.compose.foundation.lazy.TransformingLazyColumnItemScope.EmptyApps() {
    WearCard {
        Text(
            "未加载应用",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** 应用详情：启停、清数据、卸载都在单独一页。 */
@Composable
fun AppDetailScreen(
    repo: EngineRepository,
    state: CoreState,
    name: String,
    label: String,
    onBack: () -> Unit,
) {
    WearScreen(title = label, onBack = onBack) {
        item {
            WearCard(title = "包名") {
                Text(
                    name,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        item {
            WearButton(icon = R.drawable.ic_play, label = "启动", enabled = state.online) {
                repo.send(if (state.harmony) Cmd.hdcLaunchPackage(name) else Cmd.launchPackage(name))
            }
        }
        item {
            WearButton(icon = R.drawable.ic_stop, label = "强制停止", enabled = state.online) {
                repo.send(if (state.harmony) Cmd.hdcStopPackage(name) else Cmd.stopPackage(name))
            }
        }
        item {
            WearButton(icon = R.drawable.ic_trash, label = "清除数据", enabled = state.online) {
                if (state.harmony) {
                    repo.send(Cmd.hdcClearAppData(name, false))
                } else {
                    repo.send(Cmd.clearPackage(name))
                }
            }
        }
        item {
            WearButton(icon = R.drawable.ic_close, label = "卸载", enabled = state.online) {
                repo.send(if (state.harmony) Cmd.hdcUninstallPackage(name) else Cmd.uninstallPackage(name))
            }
        }
    }
}
