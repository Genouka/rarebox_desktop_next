package com.rarebox.wear

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.window.Dialog
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.Text
import com.rarebox.wear.core.AdbDiscovery
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.ConfirmRequest
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.core.SafImport
import com.rarebox.wear.core.ToastMessage
import com.rarebox.wear.ui.RareBoxTheme
import com.rarebox.wear.ui.Route
import com.rarebox.wear.ui.screens.AppsScreen
import com.rarebox.wear.ui.screens.AppDetailScreen
import com.rarebox.wear.ui.screens.ConnectDevicesScreen
import com.rarebox.wear.ui.screens.ConnectHubScreen
import com.rarebox.wear.ui.screens.ConnectPairScreen
import com.rarebox.wear.ui.screens.ConnectScanScreen
import com.rarebox.wear.ui.screens.ConnectUsbScreen
import com.rarebox.wear.ui.screens.ConnectWlanScreen
import com.rarebox.wear.ui.screens.ConnectWirelessScreen
import com.rarebox.wear.ui.screens.DisplayParamsScreen
import com.rarebox.wear.ui.screens.DisplayScreen
import com.rarebox.wear.ui.screens.MoreScreen
import com.rarebox.wear.ui.screens.ScreenshotScreen
import com.rarebox.wear.ui.screens.SettingsAboutScreen
import com.rarebox.wear.ui.screens.SettingsLegalScreen
import com.rarebox.wear.ui.screens.SettingsToolsScreen
import com.rarebox.wear.ui.screens.FilesScreen
import com.rarebox.wear.ui.screens.HomeScreen
import com.rarebox.wear.ui.screens.LogsScreen
import com.rarebox.wear.ui.screens.OverviewScreen
import com.rarebox.wear.ui.screens.RemoteScreen
import com.rarebox.wear.ui.screens.SettingsScreen
import com.rarebox.wear.ui.screens.TerminalScreen
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

class MainActivity : ComponentActivity() {

    private val repository by lazy { EngineRepository(applicationContext) }
    private val discovery by lazy { AdbDiscovery(applicationContext) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            RareBoxTheme {
                RareBoxApp(repository, discovery)
            }
        }
    }
}

@Composable
private fun RareBoxApp(repo: EngineRepository, discovery: AdbDiscovery) {
    val context = LocalContext.current
    val state by repo.state.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()

    var stack by remember { mutableStateOf(listOf<Route>(Route.Home)) }
    val route = stack.last()
    fun navigate(target: Route) {
        if (target == Route.Home) stack = listOf(Route.Home) else stack = stack + target
    }
    fun back() {
        if (stack.size > 1) stack = stack.dropLast(1)
    }

    var toast by remember { mutableStateOf<ToastMessage?>(null) }
    var confirm by remember { mutableStateOf<ConfirmRequest?>(null) }
    var pendingPurpose by remember { mutableStateOf<String?>(null) }

    val apkPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        val purpose = pendingPurpose
        pendingPurpose = null
        if (uris.isEmpty() || purpose == null) return@rememberLauncherForActivityResult
        importAndRun(context, repo, uris, purpose) { toast = it }
    }
    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        val purpose = pendingPurpose
        pendingPurpose = null
        if (uris.isEmpty() || purpose == null) return@rememberLauncherForActivityResult
        importAndRun(context, repo, uris, purpose) { toast = it }
    }
    LaunchedEffect(Unit) {
        repo.toasts.collect { toast = it }
    }
    LaunchedEffect(Unit) {
        repo.confirms.collect { confirm = it }
    }
    LaunchedEffect(Unit) {
        repo.picks.collect { pick ->
            when (pick.purpose) {
                "install_apk" -> {
                    pendingPurpose = "install_apk"
                    apkPicker.launch(arrayOf("application/vnd.android.package-archive", "*/*"))
                }
                "install_hap" -> {
                    pendingPurpose = "install_hap"
                    filePicker.launch(arrayOf("*/*"))
                }
                "push_files", "push_hdc_files" -> {
                    pendingPurpose = pick.purpose
                    filePicker.launch(arrayOf("*/*"))
                }
                else -> toast = ToastMessage("手表端暂不支持该文件选择")
            }
        }
    }
    LaunchedEffect(Unit) {
        repo.blocks.collect { block ->
            when (block.title) {
                "打开链接" -> openUrl(context, block.body)
                "文件位置" -> sharePath(context, block.body)
                "已复制" -> copyText(context, block.body)
            }
        }
    }

    LaunchedEffect(toast) {
        if (toast != null) {
            delay(2600)
            toast = null
        }
    }

    BackHandler(enabled = stack.size > 1) { back() }

    val screen: @Composable () -> Unit = {
        when (route) {
            Route.Home -> HomeScreen(state) { navigate(it) }
            Route.More -> MoreScreen(state, onNavigate = ::navigate, onBack = ::back)

            Route.Connect -> ConnectHubScreen(repo, state, onNavigate = ::navigate, onBack = ::back)
            Route.ConnectWireless -> ConnectWirelessScreen(repo, state, onNavigate = ::navigate, onBack = ::back)
            Route.ConnectPair -> ConnectPairScreen(repo, state, onBack = ::back)
            Route.ConnectWlan -> ConnectWlanScreen(repo, state, onBack = ::back)
            Route.ConnectUsb -> ConnectUsbScreen(repo, state, onBack = ::back)
            Route.ConnectScan -> ConnectScanScreen(repo, state, onNavigate = ::navigate, onBack = ::back)
            Route.ConnectDevices -> ConnectDevicesScreen(repo, state, onBack = ::back)

            Route.Overview -> OverviewScreen(repo, state, onBack = ::back)
            Route.Files -> FilesScreen(
                repo,
                state,
                onBack = ::back,
                onPickUpload = { repo.send(if (state.harmony) Cmd.pickPushHdcFiles() else Cmd.pickPushFiles()) },
            )
            Route.Apps -> AppsScreen(
                repo,
                state,
                onBack = ::back,
                onPickInstall = { repo.send(if (state.harmony) Cmd.pickInstallHaps() else Cmd.pickInstallApks()) },
                onOpenApp = { name, label -> navigate(Route.AppDetail(name, label)) },
            )
            is Route.AppDetail -> AppDetailScreen(
                repo,
                state,
                name = route.name,
                label = route.label,
                onBack = ::back,
            )

            Route.Display -> DisplayScreen(repo, state, onNavigate = ::navigate, onBack = ::back)
            Route.Screenshot -> ScreenshotScreen(
                repo,
                state,
                onBack = ::back,
                onShare = { path -> sharePath(context, path) },
            )
            Route.DisplayParams -> DisplayParamsScreen(repo, state, onBack = ::back)

            Route.Remote -> RemoteScreen(repo, state, repo.frames, onBack = ::back)
            Route.Terminal -> TerminalScreen(repo, state, onBack = ::back)
            Route.Logs -> LogsScreen(repo, state, onBack = ::back)

            Route.Settings -> SettingsScreen(repo, state, onNavigate = ::navigate, onBack = ::back)
            Route.SettingsAbout -> SettingsAboutScreen(state, onBack = ::back)
            Route.SettingsTools -> SettingsToolsScreen(repo, state, onBack = ::back)
            Route.SettingsLegal -> SettingsLegalScreen(onBack = ::back, onOpenUrl = { openUrl(context, it) })
        }
    }

    Box(Modifier.fillMaxSize()) {
        if (stack.size > 1) {
            androidx.wear.compose.material3.SwipeToDismissBox(onDismissed = ::back) { isBackground ->
                if (isBackground) {
                    Box(
                        Modifier
                            .fillMaxSize()
                            .background(MaterialTheme.colorScheme.surfaceContainer),
                    )
                } else {
                    screen()
                }
            }
        } else {
            screen()
        }

        toast?.let { message ->
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                androidx.wear.compose.material3.Card(
                    modifier = Modifier.padding(horizontal = 22.dp),
                ) {
                    Text(
                        text = message.text,
                        fontSize = 12.sp,
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        color = when (message.success) {
                            true -> Color(0xFF9BE7B0)
                            false -> Color(0xFFFF8A80)
                            null -> MaterialTheme.colorScheme.onSurface
                        },
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }
        }
    }

    confirm?.let { request ->
        ConfirmDialog(
            request = request,
            onConfirm = {
                repo.send(Cmd.executeConfirm(request.kind))
                confirm = null
            },
            onDismiss = { confirm = null },
        )
    }
}

@Composable
private fun ConfirmDialog(request: ConfirmRequest, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    Dialog(onDismissRequest = onDismiss) {
        androidx.wear.compose.material3.Card {
            androidx.compose.foundation.layout.Column(
                modifier = Modifier.fillMaxWidth().padding(10.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(request.title, fontSize = 13.sp, color = MaterialTheme.colorScheme.primary)
                Text(request.message, fontSize = 10.sp, modifier = Modifier.padding(vertical = 6.dp))
                androidx.wear.compose.material3.Button(onClick = onConfirm) {
                    Text(request.confirmLabel)
                }
                androidx.wear.compose.material3.Button(onClick = onDismiss) {
                    Text("取消")
                }
            }
        }
    }
}

private fun importAndRun(
    context: Context,
    repo: EngineRepository,
    uris: List<Uri>,
    purpose: String,
    notify: (ToastMessage) -> Unit,
) {
    val activity = context as? ComponentActivity ?: return
    activity.lifecycleScope.launch {
        val files = withContext(Dispatchers.IO) {
            uris.mapNotNull { uri -> SafImport.copy(context, uri) }
        }
        if (files.isEmpty()) {
            notify(ToastMessage("没有读取到文件", false))
            return@launch
        }
        val paths = files.map { it.absolutePath }
        // 让引擎按「当前目录」上传；path 列表只包含真实文件。
        when (purpose) {
            "install_apk" -> repo.send(Cmd.installApks(paths))
            "install_hap" -> repo.send(Cmd.hdcInstallHaps(paths))
            "push_hdc_files" -> repo.send(Cmd.hdcUploadFiles(paths))
            else -> repo.send(Cmd.pushFiles(paths))
        }
        notify(ToastMessage("已选择 ${files.size} 个文件", true))
    }
}

private fun sharePath(context: Context, path: String) {
    val file = File(path)
    if (!file.exists()) {
        Toast.makeText(context, "文件不存在", Toast.LENGTH_SHORT).show()
        return
    }
    runCatching {
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        val intent = Intent(Intent.ACTION_SEND).apply {
            type = context.contentResolver.getType(uri) ?: "*/*"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        context.startActivity(Intent.createChooser(intent, "分享").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }.onFailure { Toast.makeText(context, "无法分享：${it.message}", Toast.LENGTH_SHORT).show() }
}

private fun openUrl(context: Context, url: String) {
    runCatching {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }.onFailure { Toast.makeText(context, "手表上没有可打开链接的应用", Toast.LENGTH_SHORT).show() }
}

private fun copyText(context: Context, text: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText("RareBox", text))
    Toast.makeText(context, "已复制", Toast.LENGTH_SHORT).show()
}
