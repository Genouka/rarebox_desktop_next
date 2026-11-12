/*
 * RareBox 引擎 C ABI。
 *
 * 协议：
 *   - 命令：rarebox_engine_send(handle, "{\"cmd\":\"refresh_devices\"}")
 *   - 事件：JSON 回调（Event::to_protocol_json）；画面帧走独立 BGRA 回调
 *   - 状态：rarebox_engine_state(handle) 返回 camelCase JSON 快照
 *
 * 线程：命令与状态查询可以在任意线程调用；回调在引擎事件线程执行。
 * 所有返回的字符串都需要 rarebox_string_free 释放。
 */
#ifndef RAREBOX_H
#define RAREBOX_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct EngineHandle EngineHandle;

typedef void (*rarebox_json_callback)(const char *json);
typedef void (*rarebox_frame_callback)(uint64_t generation, uint32_t width,
                                       uint32_t height, const uint8_t *bgra,
                                       size_t len);

/* 使用本地配置创建引擎。 */
EngineHandle *rarebox_engine_new(void);

/* 使用 JSON 配置创建引擎；config_json 可为 NULL。 */
EngineHandle *rarebox_engine_new_with_config(const char *config_json);

/* Android：设置 applicationInfo.nativeLibraryDir；须在 rarebox_engine_new* 前调用。 */
void rarebox_engine_set_native_lib_dir(const char *dir);

/* Android / Wear：设置数据、图片、下载目录；须在 rarebox_engine_new* 前调用。
 * 空指针或空字符串表示跳过。 */
void rarebox_engine_set_app_dirs(const char *data_dir, const char *pictures_dir,
                                 const char *downloads_dir);

void rarebox_engine_free(EngineHandle *handle);

/* 投递 JSON 命令；JSON 无效或引擎已关闭时返回 false。 */
bool rarebox_engine_send(EngineHandle *handle, const char *json);

/* 读取最近一次状态快照 JSON；调用方负责 rarebox_string_free。 */
char *rarebox_engine_state(EngineHandle *handle);

/* 设置回调；任一参数传 NULL 取消。 */
void rarebox_engine_set_callbacks(EngineHandle *handle,
                                  rarebox_json_callback json,
                                  rarebox_frame_callback frame);

void rarebox_string_free(char *text);

#ifdef __cplusplus
}
#endif

#endif /* RAREBOX_H */
