package com.rarebox.wear.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.wear.compose.foundation.lazy.TransformingLazyColumnItemScope
import androidx.wear.compose.material3.Icon
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.Text
import androidx.wear.compose.material3.lazy.rememberTransformationSpec
import androidx.wear.compose.material3.lazy.transformedHeight
import androidx.wear.compose.material3.SurfaceTransformation
import com.rarebox.wear.R
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.ui.IconGrid
import com.rarebox.wear.ui.LocalTransformationSpec
import com.rarebox.wear.ui.Route
import com.rarebox.wear.ui.WearButton
import com.rarebox.wear.ui.WearCard
import com.rarebox.wear.ui.WearScreen
import androidx.compose.ui.res.painterResource

@Composable
fun HomeScreen(
    state: CoreState,
    onNavigate: (Route) -> Unit,
) {
    WearScreen(title = "RareBox") {
        item {
            DeviceCard(state = state, onClick = { onNavigate(Route.Connect) })
        }
        item {
            IconGrid(
                entries = listOf(
                    Triple(R.drawable.ic_connect, "连接", { onNavigate(Route.Connect) }),
                    Triple(R.drawable.ic_overview, "概览", { onNavigate(Route.Overview) }),
                    Triple(R.drawable.ic_apps, "应用", { onNavigate(Route.Apps) }),
                    Triple(R.drawable.ic_files, "文件", { onNavigate(Route.Files) }),
                ),
            )
        }
        item {
            WearButton(icon = R.drawable.ic_more, label = "更多") { onNavigate(Route.More) }
        }
        if (!state.online) {
            item {
                WearCard {
                    Text(
                        "未连接设备。点上方「连接」开启无线调试或插线。",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

/** 顶部设备卡：图标 + 名称 + 在线状态 + 系统标记。 */
@Composable
fun TransformingLazyColumnItemScope.DeviceCard(
    state: CoreState,
    onClick: (() -> Unit)? = null,
) {
    val colors = MaterialTheme.colorScheme
    val online = state.online
    WearCard(onClick = onClick) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                painter = painterResource(R.drawable.ic_watch),
                contentDescription = null,
                tint = if (online) colors.primary else colors.outline,
                modifier = Modifier.size(30.dp),
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = state.currentName ?: "未连接设备",
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = (if (state.harmony) "HarmonyOS" else "Android") +
                        " · " + if (online) "在线" else "离线",
                    style = MaterialTheme.typography.bodySmall,
                    color = if (online) colors.tertiary else colors.onSurfaceVariant,
                )
                state.activity?.let { activity ->
                    Text(
                        text = activity,
                        style = MaterialTheme.typography.labelSmall,
                        color = colors.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}
