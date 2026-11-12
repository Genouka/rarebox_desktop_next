package com.rarebox.wear.core

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.RectF
import android.util.Log
import com.rarebox.core.CoreEngine
import com.rarebox.core.CoreListener
import com.rarebox.wear.ui.screens.RemoteTextureView
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.nio.ByteBuffer

data class ToastMessage(val text: String, val success: Boolean? = null)

data class ConfirmRequest(
    val title: String,
    val message: String,
    val confirmLabel: String,
    val kind: JsonElement,
)

data class PickRequest(val purpose: String)

/** 引擎句柄的所有权与事件分发：Compose 只读 StateFlow，不直接触碰 JNI。 */
class EngineRepository(context: Context) {

    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val _state = MutableStateFlow(CoreState())
    val state = _state.asStateFlow()

    private val _toasts = MutableSharedFlow<ToastMessage>(extraBufferCapacity = 32, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val toasts = _toasts.asSharedFlow()

    private val _confirms = MutableSharedFlow<ConfirmRequest>(extraBufferCapacity = 8, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val confirms = _confirms.asSharedFlow()

    private val _picks = MutableSharedFlow<PickRequest>(extraBufferCapacity = 8, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val picks = _picks.asSharedFlow()

    private val _blocks = MutableSharedFlow<BlockMessage>(extraBufferCapacity = 16, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val blocks = _blocks.asSharedFlow()

    val frames = FrameStore()
    private val refreshSignal = MutableSharedFlow<Unit>(extraBufferCapacity = 1, onBufferOverflow = BufferOverflow.DROP_OLDEST)

    private val engine: CoreEngine = CoreEngine.create(appContext, configJson())
    private val listener = object : CoreListener {
        override fun onEvent(json: String) = handleEvent(json)

        override fun onFrame(generation: Long, width: Int, height: Int, bgra: ByteArray) {
            frames.submit(generation, width, height, bgra)
        }
    }

    init {
        engine.startEvents(listener)
        scope.launch {
            refreshSignal.collectLatest {
                // 合并高频 changed：状态快照包含日志/终端等大数组，80ms 一次足够。
                delay(80)
                refreshNow()
            }
        }
        send(Cmd.refreshDevices())
        send(Cmd.refreshHdc())
        refreshSignal.tryEmit(Unit)
    }

    fun send(command: String) {
        if (!engine.send(command)) {
            Log.w(TAG, "command rejected: $command")
            _toasts.tryEmit(ToastMessage("命令发送失败", success = false))
        }
    }

    fun refreshNow() {
        val raw = runCatching { engine.stateJson() }.getOrElse {
            Log.w(TAG, "stateJson failed", it)
            return
        }
        val parsed = runCatching { CoreJson.decodeFromString<CoreState>(raw) }.getOrElse {
            Log.w(TAG, "state parse failed", it)
            return
        }
        _state.value = parsed
    }

    private fun handleEvent(json: String) {
        val obj = runCatching { CoreJson.parseToJsonElement(json).jsonObject }.getOrNull() ?: return
        when (obj["event"]?.jsonPrimitive?.content) {
            "changed" -> refreshSignal.tryEmit(Unit)
            "toast" -> _toasts.tryEmit(
                ToastMessage(
                    obj["message"]?.jsonPrimitive?.content.orEmpty(),
                    success = when (obj["kind"]?.jsonPrimitive?.content) {
                        "success" -> true
                        "error" -> false
                        else -> null
                    },
                ),
            )
            "confirm" -> {
                val kind = obj["kind"] ?: return
                _confirms.tryEmit(
                    ConfirmRequest(
                        title = obj["title"]?.jsonPrimitive?.content.orEmpty(),
                        message = obj["message"]?.jsonPrimitive?.content.orEmpty(),
                        confirmLabel = obj["confirmLabel"]?.jsonPrimitive?.content ?: "确认",
                        kind = kind,
                    ),
                )
            }
            "pick_files" -> _picks.tryEmit(
                PickRequest(obj["purpose"]?.jsonPrimitive?.content.orEmpty()),
            )
            "pick_directory" -> _toasts.tryEmit(ToastMessage("手表端使用默认目录"))
            "open_url" -> _blocks.tryEmit(
                BlockMessage("打开链接", obj["url"]?.jsonPrimitive?.content.orEmpty()),
            )
            "open_path" -> _blocks.tryEmit(
                BlockMessage("文件位置", obj["path"]?.jsonPrimitive?.content.orEmpty()),
            )
            "copy_text" -> _blocks.tryEmit(
                BlockMessage("已复制", obj["text"]?.jsonPrimitive?.content.orEmpty()),
            )
            "video_ended" -> {
                _toasts.tryEmit(ToastMessage("投屏已断开", success = false))
                frames.clear()
                refreshSignal.tryEmit(Unit)
            }
            "legal" -> Unit
            "quit" -> _toasts.tryEmit(ToastMessage("引擎要求退出应用", success = false))
        }
    }

    private fun configJson(): String = buildJsonObject {
        put("dark", true)
        put("seed", 0x69_6D)
        // 手表屏幕小：限制解码尺寸与码率，降低内存与电量开销。
        put("remote_max_size", 512)
        put("remote_bitrate", 2_000_000)
        put("kill_processes_on_exit", true)
    }.toString()

    companion object {
        private const val TAG = "RareBoxWear"
    }
}

/** 供宿主展示但无法内嵌处理的系统级消息（链接、路径、剪贴板）。 */
data class BlockMessage(val title: String, val body: String)

/** 共享的画面缓冲：回调线程写入，UI 线程绘制，只保留最新一帧。 */
class FrameStore {
    private val lock = Any()
    private var bitmap: Bitmap? = null
    private var width = 0
    private var height = 0
    private var view: RemoteTextureView? = null

    @Volatile
    var attached = false

    fun attach(target: RemoteTextureView) {
        view = target
        attached = true
    }

    fun detach(target: RemoteTextureView) {
        if (view === target) {
            view = null
            attached = false
        }
    }

    fun submit(generation: Long, frameWidth: Int, frameHeight: Int, bgra: ByteArray) {
        if (!attached || frameWidth <= 1 || frameHeight <= 1) return
        if (bgra.size < frameWidth * frameHeight * 4) return
        synchronized(lock) {
            if (bitmap == null || width != frameWidth || height != frameHeight) {
                bitmap = Bitmap.createBitmap(frameWidth, frameHeight, Bitmap.Config.ARGB_8888)
                width = frameWidth
                height = frameHeight
            }
            bitmap?.copyPixelsFromBuffer(ByteBuffer.wrap(bgra))
        }
        view?.postInvalidateOnAnimation()
    }

    fun size(): Pair<Int, Int> = synchronized(lock) { width to height }

    fun draw(canvas: Canvas, dest: RectF) {
        synchronized(lock) {
            bitmap?.let { canvas.drawBitmap(it, null, dest, null) }
        }
    }

    fun hasFrame(): Boolean = synchronized(lock) { bitmap != null }

    fun clear() {
        synchronized(lock) {
            bitmap = null
            width = 0
            height = 0
        }
    }
}
