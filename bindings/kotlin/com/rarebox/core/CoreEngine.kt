package com.rarebox.core

/**
 * RareBox 核心引擎的 Kotlin 绑定。
 *
 * 原生库是 `crates/rarebox-ffi`（产物 `librarebox_ffi.so`）。协议完全由 JSON 描述：
 *
 * - 命令：`send("""{"cmd":"refresh_devices"}""")`
 * - 事件：`CoreListener.onEvent(json)`，`Changed` 事件后调用 [stateJson] 取快照
 * - 画面：`CoreListener.onFrame(generation, width, height, bgra)`，BGRA8888 原始像素
 *
 * 引擎在单个后台线程串行执行命令；[send]、[stateJson] 可从任意线程调用。
 * 回调运行在原生事件线程，Compose 侧请自行切到主线程（例如 `withContext(Dispatchers.Main)`）。
 */
class CoreEngine private constructor(private var handle: Long) : AutoCloseable {

    companion object {
        init {
            System.loadLibrary("rarebox_ffi")
        }

        /**
         * 内置 adb / hdc 从 APK 的 `nativeLibraryDir` 执行；必须在 [create] 之前设置。
         * Android 上可直接用 `applicationInfo.nativeLibraryDir`。
         */
        fun setNativeLibraryDir(dir: String) {
            CoreNative.setNativeLibraryDir(dir)
        }

        /** Android 便捷入口：自动注入 `nativeLibraryDir` 后创建引擎。 */
        fun create(context: android.content.Context): CoreEngine {
            setNativeLibraryDir(context.applicationInfo.nativeLibraryDir)
            return create()
        }

        /** 使用应用自己的配置文件（adb/hdc 路径、主题等）创建引擎。 */
        fun create(): CoreEngine = CoreEngine(nativeNew())

        /**
         * 用显式 JSON 配置创建引擎：
         * `{"adb_path":"","hdc_path":"","dark":false,"seed":26997,...}`。
         * 字段与 `rarebox_core::config::Config` 的 serde 名称一致。
         */
        fun create(configJson: String): CoreEngine = CoreEngine(nativeNewWithConfig(configJson))
    }

    /** 投递一条 JSON 命令；返回 false 表示 JSON 无效或引擎已关闭。 */
    fun send(commandJson: String): Boolean {
        val handle = handle
        return handle != 0L && nativeSend(handle, commandJson)
    }

    /** 最近一次发布的状态快照（camelCase JSON）。 */
    fun stateJson(): String {
        val handle = handle
        return if (handle == 0L) "{}" else (nativeState(handle) ?: "{}")
    }

    /** 订阅事件与画面帧。建议只调用一次。 */
    fun startEvents(listener: CoreListener) {
        val handle = handle
        if (handle != 0L) {
            nativeStartEvents(handle, listener)
        }
    }

    override fun close() {
        val handle = handle
        handle = 0L
        if (handle != 0L) {
            nativeFree(handle)
        }
    }

    private external fun nativeNew(): Long
    private external fun nativeNewWithConfig(configJson: String): Long
    private external fun nativeSend(handle: Long, commandJson: String): Boolean
    private external fun nativeState(handle: Long): String?
    private external fun nativeFree(handle: Long)
    private external fun nativeStartEvents(handle: Long, listener: CoreListener)
}

/**
 * 不依赖引擎实例的原生入口。`object` 的实例方法对应 JNI
 * `Java_com_rarebox_core_CoreNative_setNativeLibraryDir`。
 */
object CoreNative {
    init {
        System.loadLibrary("rarebox_ffi")
    }

    /** 必须在创建引擎之前调用；Android 上传入 `applicationInfo.nativeLibraryDir`。 */
    external fun setNativeLibraryDir(dir: String)
}

/**
 * 原生引擎回调。实现方通常把 JSON 交给 `kotlinx.serialization` / `org.json` 解析。
 *
 * `onEvent` 可能非常频繁（尤其 `changed`），不要在里面做重活。
 */
interface CoreListener {
    /** 一条协议事件：`changed`、`toast`、`confirm`、`pick_files`、`video_ended` 等。 */
    fun onEvent(json: String)

    /**
     * 一帧远程画面。`bgra` 为 `width * height * 4` 字节的 BGRA8888。
     * `generation` 与状态快照里的 `remote.generation` 对应，可用于丢弃旧会话的帧。
     */
    fun onFrame(generation: Long, width: Int, height: Int, bgra: ByteArray) {}
}
