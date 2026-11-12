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
fun FilesScreen(
    repo: EngineRepository,
    state: CoreState,
    onBack: () -> Unit,
    onPickUpload: () -> Unit,
) {
    val files = if (state.harmony) state.harmonyFiles else state.files

    LaunchedEffect(Unit) {
        repo.send(if (state.harmony) Cmd.hdcLoadFiles() else Cmd.listFiles())
    }

    WearScreen(title = "文件", onBack = onBack) {
        item {
            WearCard(title = "当前目录") {
                Text(
                    files.path,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_arrow_up, label = "上级目录", enabled = state.online) {
                repo.send(if (state.harmony) Cmd.hdcGoParentFiles() else Cmd.goParentFiles())
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_refresh, label = "刷新", enabled = state.online) {
                repo.send(if (state.harmony) Cmd.hdcLoadFiles() else Cmd.listFiles())
            }
        }
        item {
            WearCompact(icon = R.drawable.ic_upload, label = "上传到这里", enabled = state.online) { onPickUpload() }
        }

        if (files.entries.isEmpty()) {
            item {
                WearCard {
                    Text(
                        "空目录或尚未加载",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        files.entries.take(200).forEach { entry ->
            item {
                WearRow(
                    icon = if (entry.dir) R.drawable.ic_folder else R.drawable.ic_logs,
                    label = entry.name,
                    value = if (entry.dir) "目录" else entry.sizeLabel,
                    onClick = {
                        val path = joinPath(files.path, entry.name)
                        if (entry.dir) {
                            repo.send(
                                if (state.harmony) Cmd.hdcNavigateFiles(path) else Cmd.navigateFiles(path),
                            )
                        } else {
                            repo.send(if (state.harmony) Cmd.hdcPullFile(path) else Cmd.pullFile(path))
                        }
                    },
                )
            }
        }

        state.lastExport?.let { path ->
            item {
                WearCard(title = "最近下载") {
                    Text(
                        path.substringAfterLast('/'),
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 2,
                    )
                }
            }
        }
    }
}

private fun joinPath(base: String, name: String): String =
    if (base.endsWith('/')) "$base$name" else "$base/$name"
