/*
 * Copyright (C) 2021 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#ifndef __H_HDC_LOG_H__
#define __H_HDC_LOG_H__

#include <atomic>
#include <cinttypes>

namespace Hdc {

namespace Base {
    extern std::atomic<uint8_t> g_logLevel;
 
    inline bool IsLoggable(uint8_t logLevel) noexcept
    {
        return logLevel <= g_logLevel.load(std::memory_order_relaxed);
    }

    void PrintLogEx(const char *functionName, int line, uint8_t logLevel, const char *msg, ...);
}

enum HdcLogLevel {
    LOG_OFF,
    LOG_FATAL,
    LOG_WARN,
    LOG_INFO,  // default
    LOG_DEBUG,
    LOG_ALL,
    LOG_VERBOSE,
    LOG_LAST = LOG_VERBOSE,  // tail, not use
};

#ifdef IS_RELEASE_VERSION
#define HDC_LOG_FUNCTION_NAME __FUNCTION__
#else
#define HDC_LOG_FUNCTION_NAME __FILE_NAME__
#endif

#define WRITE_LOG(level, fmt, ...) do { \
    const uint8_t hdcLogLevel = static_cast<uint8_t>(level); \
    if (Base::IsLoggable(hdcLogLevel)) { \
        Base::PrintLogEx(HDC_LOG_FUNCTION_NAME, __LINE__, hdcLogLevel, fmt, ##__VA_ARGS__); \
    } \
} while (0)

#ifndef HDC_HOST
#define WRITE_LOG_DAEMON(level, fmt, ...) WRITE_LOG(level, fmt, ##__VA_ARGS__)
#else
#define WRITE_LOG_DAEMON(level, fmt, ...)
#endif

#ifdef HDC_DEBUG
#define DEBUG_LOG(fmt, ...)   WRITE_LOG(LOG_DEBUG, fmt, ##__VA_ARGS__)
#else
#define DEBUG_LOG(fmt, ...)
#endif

}  // namespace Hdc
#endif  // __H_HDC_LOG_H__
