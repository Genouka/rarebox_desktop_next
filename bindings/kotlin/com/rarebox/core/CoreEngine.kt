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

        /**
         * Android / Wear 便捷入口：注入 `nativeLibraryDir` 与应用目录后创建引擎。
         *
         * 应用目录必须在创建前注入，否则配置文件、scrcpy-server 解包与
         * 截图/下载会落到不可写路径。`create(configJson)` 不会自动注入。
         */
        fun create(context: android.content.Context): CoreEngine {
            injectDirs(context)
            return create()
        }

        /** 与 [create] 相同，但使用显式 JSON 配置（手表可在此降低投屏画质）。 */
        fun create(context: android.content.Context, configJson: String): CoreEngine {
            injectDirs(context)
            return create(configJson)
        }

        private fun injectDirs(context: android.content.Context) {
            setNativeLibraryDir(context.applicationInfo.nativeLibraryDir)
            CoreNative.setAppDirs(
                context.filesDir.absolutePath,
                (context.getExternalFilesDir("Pictures") ?: context.filesDir).absolutePath,
                (context.getExternalFilesDir("Download") ?: context.filesDir).absolutePath,
            )
        }

        /** 使用应用自己的配置文件（adb/hdc 路径、主题等）创建引擎。 */
        fun create(): CoreEngine = CoreEngine(nativeNew())

        /**
         * 用显式 JSON 配置创建引擎：
         * `{"adb_path":"","hdc_path":"","dark":false,"seed":26997,...}`。
         * 字段与 `rarebox_core::config::Config` 的 serde 名称一致。
         */
        fun create(configJson: String): CoreEngine =
            CoreEngine(nativeNewWithConfig(configJson))

        @JvmStatic
        private external fun nativeNew(): Long

        @JvmStatic
        private external fun nativeNewWithConfig(configJson: String): Long
    }

    /** 投递一条 JSON 命令；返回 false 表示 JSON 无效或引擎已关闭。 */
    fun send(commandJson: String): Boolean {
        val current = handle
        return current != 0L && nativeSend(current, commandJson)
    }

    /** 最近一次发布的状态快照（camelCase JSON）。 */
    fun stateJson(): String {
        val current = handle
        return if (current == 0L) "{}" else (nativeState(current) ?: "{}")
    }

    /** 订阅事件与画面帧。建议只调用一次。 */
    fun startEvents(listener: CoreListener) {
        val current = handle
        if (current != 0L) {
            nativeStartEvents(current, listener)
        }
    }

    override fun close() {
        val current = handle
        handle = 0L
        if (current != 0L) {
            nativeFree(current)
        }
    }

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

    /**
     * 必须在创建引擎之前调用；传入应用的数据、图片与下载目录。
     *
     * 与 `rarebox_engine_set_app_dirs` 对应，空字符串表示跳过该项。
     */
    external fun setAppDirs(dataDir: String, picturesDir: String, downloadsDir: String)
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
