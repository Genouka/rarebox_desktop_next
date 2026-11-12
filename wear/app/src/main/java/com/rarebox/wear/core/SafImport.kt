package com.rarebox.wear.core

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.util.UUID

/**
 * 把 SAF 文档复制到应用缓存，返回真实路径。
 *
 * 引擎只接受本地路径（`install_apks` / `push_files`），`content://` 不能直接使用；
 * 同时这样做可以在临时 URI 授权失效后继续读取。
 */
object SafImport {
    fun copy(context: Context, uri: Uri): File? = runCatching {
        val name = displayName(context, uri)
        val dir = File(context.cacheDir, "import-${UUID.randomUUID()}")
        if (!dir.mkdirs()) error("无法创建导入目录")
        val file = File(dir, name)
        context.contentResolver.openInputStream(uri)?.use { input ->
            file.outputStream().use { output -> input.copyTo(output) }
        } ?: error("无法读取所选文件")
        file
    }.onFailure {
        android.util.Log.w("SafImport", "import failed: $uri", it)
    }.getOrNull()

    private fun displayName(context: Context, uri: Uri): String {
        var name: String? = null
        runCatching {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { cursor ->
                    if (cursor.moveToFirst()) name = cursor.getString(0)
                }
        }
        return (name ?: uri.lastPathSegment ?: "file")
            .replace('/', '_')
            .replace('\\', '_')
            .replace("\u0000", "_")
            .ifBlank { "file" }
    }
}
