package com.rarebox.wear.ui.screens

import android.content.Context
import android.graphics.Canvas
import android.graphics.RectF
import android.view.View
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.wear.compose.material3.Text
import androidx.wear.compose.material3.ripple
import androidx.compose.runtime.remember
import com.rarebox.wear.core.Cmd
import com.rarebox.wear.core.CoreState
import com.rarebox.wear.core.EngineRepository
import com.rarebox.wear.core.FrameStore
import kotlin.math.min

/** 直接绘制 BGRA→Bitmap 的画面，避免每帧触发 Compose 重组。 */
class RemoteTextureView(context: Context, private val store: FrameStore) : View(context) {
    init {
        setBackgroundColor(android.graphics.Color.BLACK)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        store.attach(this)
    }

    override fun onDetachedFromWindow() {
        store.detach(this)
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val (imgWidth, imgHeight) = store.size()
        if (imgWidth <= 0 || imgHeight <= 0) return
        store.draw(canvas, fitRect(width.toFloat(), height.toFloat(), imgWidth, imgHeight))
    }
}

fun fitRect(containerWidth: Float, containerHeight: Float, imageWidth: Int, imageHeight: Int): RectF {
    if (containerWidth <= 0f || containerHeight <= 0f || imageWidth <= 0 || imageHeight <= 0) {
        return RectF()
    }
    val scale = min(containerWidth / imageWidth, containerHeight / imageHeight)
    val drawWidth = imageWidth * scale
    val drawHeight = imageHeight * scale
    val left = (containerWidth - drawWidth) / 2f
    val top = (containerHeight - drawHeight) / 2f
    return RectF(left, top, left + drawWidth, top + drawHeight)
}

private fun mapPoint(
    position: Offset,
    containerWidth: Float,
    containerHeight: Float,
    image: Pair<Int, Int>,
): Pair<Int, Int>? {
    val (imageWidth, imageHeight) = image
    if (imageWidth <= 1 || imageHeight <= 1) return null
    val rect = fitRect(containerWidth, containerHeight, imageWidth, imageHeight)
    if (rect.isEmpty || !rect.contains(position.x, position.y)) return null
    val x = ((position.x - rect.left) / rect.width() * imageWidth).toInt().coerceIn(0, imageWidth - 1)
    val y = ((position.y - rect.top) / rect.height() * imageHeight).toInt().coerceIn(0, imageHeight - 1)
    return x to y
}

@Composable
fun RemoteScreen(
    repo: EngineRepository,
    state: CoreState,
    frames: FrameStore,
    onBack: () -> Unit,
) {
    val remote = state.remote
    val view = LocalView.current
    val imageSize = remote.width to remote.height

    DisposableEffect(view) {
        view.keepScreenOn = true
        if (!remote.running) repo.send(Cmd.startRemote())
        onDispose {
            view.keepScreenOn = false
            repo.send(Cmd.stopRemote())
        }
    }

    var showText by remember { mutableStateOf(false) }
    var text by remember { mutableStateOf("") }

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        AndroidView(
            factory = { context -> RemoteTextureView(context, frames) },
            modifier = Modifier
                .fillMaxSize()
                .pointerInput(imageSize) {
                    if (imageSize.first > 1) injectTouch(repo, imageSize)
                },
        )

        val phaseText = when (remote.phase) {
            "starting" -> "正在启动投屏…"
            "running" -> "${remote.width}×${remote.height}"
            "failed" -> remote.error ?: "启动失败"
            else -> "未启动"
        }
        Text(
            text = phaseText,
            fontSize = 10.sp,
            color = if (remote.phase == "failed") Color(0xFFFF8A80) else Color(0xFF7AD7F0),
            maxLines = 1,
            modifier = Modifier.align(Alignment.TopCenter).padding(top = 6.dp),
        )

        Row(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(Color(0xCC000000))
                .padding(horizontal = 6.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RemoteKey("返回") { repo.send(Cmd.sendRemoteKey(4)) }
            RemoteKey("主页") { repo.send(Cmd.sendRemoteKey(3)) }
            RemoteKey("最近") { repo.send(Cmd.sendRemoteKey(187)) }
            RemoteKey("电源") { repo.send(Cmd.sendRemoteKey(26)) }
            RemoteKey("文字") { showText = !showText }
            RemoteKey("退出", onBack)
        }

        if (showText) {
            Column(
                modifier = Modifier
                    .align(Alignment.Center)
                    .fillMaxWidth()
                    .background(Color(0xF0101820))
                    .padding(10.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                BasicTextField(
                    value = text,
                    onValueChange = { text = it },
                    textStyle = TextStyle(color = Color.White, fontSize = 13.sp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(Color(0xFF24323C))
                        .padding(6.dp),
                )
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                ) {
                    val sendInteraction = remember { MutableInteractionSource() }
                    Text(
                        "发送",
                        fontSize = 12.sp,
                        color = Color(0xFF7AD7F0),
                        modifier = Modifier.padding(8.dp).clickable(
                            interactionSource = sendInteraction,
                            indication = ripple(),
                        ) {
                            if (text.isNotBlank()) {
                                repo.send(Cmd.sendRemoteText(text))
                                text = ""
                            }
                        },
                    )
                    val closeInteraction = remember { MutableInteractionSource() }
                    Text(
                        "关闭",
                        fontSize = 12.sp,
                        color = Color(0xFF7AD7F0),
                        modifier = Modifier.padding(8.dp).clickable(
                            interactionSource = closeInteraction,
                            indication = ripple(),
                        ) { showText = false },
                    )
                }
            }
        }
    }
}

@Composable
private fun RemoteKey(label: String, onClick: () -> Unit) {
    val interaction = remember { MutableInteractionSource() }
    Text(
        text = label,
        fontSize = 11.sp,
        color = Color.White,
        modifier = Modifier.padding(horizontal = 3.dp, vertical = 8.dp).clickable(
            interactionSource = interaction,
            indication = ripple(),
            onClick = onClick,
        ),
    )
}

/** 把按压映射为设备像素坐标；与桌面端 `remote_device_point` 的等比适配一致。 */
private suspend fun PointerInputScope.injectTouch(repo: EngineRepository, image: Pair<Int, Int>) {
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false)
        val start = mapPoint(down.position, size.width.toFloat(), size.height.toFloat(), image)
            ?: return@awaitEachGesture
        repo.send(Cmd.remotePointerDown(start.first, start.second))
        down.consume()
        var last = start
        while (true) {
            val event = awaitPointerEvent()
            val change = event.changes.firstOrNull { it.id == down.id } ?: break
            val point = mapPoint(change.position, size.width.toFloat(), size.height.toFloat(), image)
            if (point != null && point != last) {
                repo.send(Cmd.remotePointerMove(point.first, point.second))
                last = point
            }
            change.consume()
            if (!change.pressed) {
                repo.send(Cmd.remotePointerUp(last.first, last.second))
                break
            }
        }
    }
}
