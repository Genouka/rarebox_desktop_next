package com.rarebox.wear.ui.screens

import androidx.compose.runtime.Composable
import com.rarebox.wear.R
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.ui.IconGrid
import com.rarebox.wear.ui.Route
import com.rarebox.wear.ui.WearScreen

/** 「更多」：放不常驻首页的功能，避免首页堆满按钮。 */
@Composable
fun MoreScreen(
    state: CoreState,
    onNavigate: (Route) -> Unit,
    onBack: () -> Unit,
) {
    WearScreen(title = "更多", onBack = onBack) {
        item {
            IconGrid(
                entries = listOf(
                    Triple(R.drawable.ic_screen, "屏幕", { onNavigate(Route.Display) }),
                    Triple(R.drawable.ic_cast, "投屏", { onNavigate(Route.Remote) }),
                    Triple(R.drawable.ic_terminal, "终端", { onNavigate(Route.Terminal) }),
                    Triple(R.drawable.ic_logs, "日志", { onNavigate(Route.Logs) }),
                ),
            )
        }
        item {
            IconGrid(
                entries = listOf(
                    Triple(R.drawable.ic_settings, "设置", { onNavigate(Route.Settings) }),
                ),
            )
        }
    }
}
