package com.rarebox.wear.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material3.Button
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.Text
import com.rarebox.wear.R
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.ui.MonoText
import com.rarebox.wear.ui.SimpleTextField
import com.rarebox.wear.ui.WearButton
import com.rarebox.wear.ui.WearCompact
import com.rarebox.wear.ui.WearCard
import com.rarebox.wear.ui.WearScreen

@Composable
fun TerminalScreen(repo: EngineRepository, state: CoreState, onBack: () -> Unit) {
    var command by remember { mutableStateOf("") }
    val entries = if (state.harmony) state.harmonyTerminal else state.terminal

    WearScreen(title = "终端", onBack = onBack) {
        item {
            WearCard {
                SimpleTextField(command, "adb shell 命令", { command = it })
            }
        }
        item {
            WearButton(
                icon = R.drawable.ic_keyboard,
                label = "执行",
                enabled = command.isNotBlank() && state.online,
            ) {
                repo.send(
                    if (state.harmony) Cmd.hdcSubmitTerminal(command) else Cmd.submitTerminal(command),
                )
                command = ""
            }
        }
        if (entries.isEmpty()) {
            item {
                WearCard {
                    Text(
                        "还没有命令记录",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        entries.takeLast(8).reversed().forEach { entry ->
            item {
                WearCard(title = if (entry.success) "✓ ${entry.command}" else "✗ ${entry.command}") {
                    MonoText(entry.output.ifBlank { "(无输出)" }, maxLines = 10)
                    Text(
                        "${entry.durationMs ?: 0} ms · ${entry.time}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
fun LogsScreen(repo: EngineRepository, state: CoreState, onBack: () -> Unit) {
    var level by remember { mutableStateOf("all") }
    val logs = if (state.harmony) state.harmonyLogs else state.logs

    LaunchedEffect(Unit) {
        if (state.harmony) repo.send(Cmd.hdcLoadLogs()) else repo.send(Cmd.loadLogs())
    }

    val filtered = logs.filter { line ->
        when (level) {
            "all" -> true
            else -> line.level.equals(level, ignoreCase = true) ||
                (level == "error" && line.level.equals("fatal", ignoreCase = true))
        }
    }

    WearScreen(title = "日志", onBack = onBack) {
        item {
            WearCompact(icon = R.drawable.ic_refresh, label = "刷新", enabled = state.online) {
                repo.send(if (state.harmony) Cmd.hdcLoadLogs() else Cmd.loadLogs())
            }
        }
        item {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                listOf("all" to "全部", "info" to "信息", "warn" to "警告", "error" to "错误")
                    .forEach { (value, label) ->
                        Button(
                            onClick = { level = value },
                            modifier = Modifier.weight(1f),
                        ) {
                            Text(
                                label,
                                style = MaterialTheme.typography.labelSmall,
                                maxLines = 1,
                                color = if (level == value) {
                                    MaterialTheme.colorScheme.onPrimary
                                } else {
                                    MaterialTheme.colorScheme.onSurfaceVariant
                                },
                            )
                        }
                    }
            }
        }
        item {
            Text(
                "${filtered.size} 条",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        filtered.takeLast(40).reversed().forEach { line ->
            item {
                WearCard {
                    Text(
                        text = "${line.level.uppercase()} ${line.tag}: ${line.message}",
                        style = MaterialTheme.typography.labelSmall,
                        maxLines = 4,
                        color = when (line.level.lowercase()) {
                            "error", "fatal" -> MaterialTheme.colorScheme.error
                            "warn" -> MaterialTheme.colorScheme.primary
                            else -> MaterialTheme.colorScheme.onSurface
                        },
                    )
                }
            }
        }
    }
}
