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
#include "tcp.h"
#include <new>

namespace Hdc {
HdcTCPBase::HdcTCPBase(const bool serverOrDaemonIn, void *ptrMainBase)
{
    // Calling the initialization
    InitialChildClass(serverOrDaemonIn, ptrMainBase);
}

HdcTCPBase::~HdcTCPBase()
{
}

// Subclasses must be explicitly called
void HdcTCPBase::InitialChildClass(const bool serverOrDaemonIn, void *ptrMainBase)
{
    serverOrDaemon = serverOrDaemonIn;
    clsMainBase = ptrMainBase;
}

void HdcTCPBase::RecvUDP(uv_udp_t *handle, ssize_t nread, const uv_buf_t *rcvbuf, const struct sockaddr *addr,
                         unsigned /* flags */)
{
    if (handle == nullptr) {
        WRITE_LOG(LOG_FATAL, "RecvUDP handle is null");
        return;
    }
    if (handle->data == nullptr) {
        WRITE_LOG(LOG_FATAL, "RecvUDP handle->data is null");
        return;
    }
    HdcTCPBase *thisClass = (HdcTCPBase *)handle->data;
    // Taken before any early return, so the error paths below are visible to the loop monitor.
    CALLSTAT_GUARD(((HdcSessionBase *)(thisClass->clsMainBase))->loopMainStatus,
                   handle->loop, "HdcTCPBase::RecvUDP");
    while (true) {
        if (nread == 0) {
            // libuv reports EAGAIN with nread == 0, this is the normal end of a drain.
            break;
        }
        if (nread < 0) {
            char errBuf[BUF_SIZE_TINY] = {0};
            uv_strerror_r(static_cast<int>(nread), errBuf, sizeof(errBuf));
            WRITE_LOG(LOG_WARN, "RecvUDP failed nread:%zd err:%s", nread, errBuf);
            // These errors are not recoverable by retrying, stop polling to avoid a busy loop.
            if (nread == UV_EPERM || nread == UV_EACCES || nread == UV_ENOTSOCK) {
                uv_udp_recv_stop(handle);
            }
            break;
        }
        if (rcvbuf == nullptr) {
            WRITE_LOG(LOG_FATAL, "RecvUDP rcvbuf is null, nread:%zd", nread);
            break;
        }
        if (rcvbuf->base == nullptr) {
            WRITE_LOG(LOG_FATAL, "RecvUDP rcvbuf->base is null, nread:%zd rcvbuf->len:%zu", nread, rcvbuf->len);
            break;
        }
        // The buffer is reused now, so terminate it here; %s and strstr() rely on it.
        const_cast<char *>(rcvbuf->base)[nread] = '\0';
        WRITE_LOG(LOG_DEBUG, "RecvUDP %s", rcvbuf->base);
        if (strncmp(rcvbuf->base, HANDSHAKE_MESSAGE.c_str(), HANDSHAKE_MESSAGE.size())) {
            break;
        }
        // nread is the payload length; hand it down so callees need not rescan the buffer.
        thisClass->RecvUDPEntry(addr, handle, rcvbuf, nread);
        break;
    }
    // The buffer is no longer heap allocated, so there is nothing to release here.
}

void HdcTCPBase::AllocStreamUDP(uv_handle_t* /* handle */, size_t /* sizeWanted */, uv_buf_t *buf)
{
    // Reused buffer: nothing to allocate, so this cannot fail and libuv never takes its
    // ENOBUFS path. The last byte is reserved for the terminator added in RecvUDP.
    // Valid only while recv_cb runs, callees must not keep the pointer.
    static thread_local char recvBuf[BUF_SIZE_DEFAULT];
    buf->base = recvBuf;
    buf->len = sizeof(recvBuf) - 1;
}

void HdcTCPBase::SendUDPFinish(uv_udp_send_t *req, int /* status */)
{
    delete req;
}

void HdcTCPBase::ReadStream(uv_stream_t *tcp, ssize_t nread, const uv_buf_t *buf)
{
    HSession hSession = (HSession)tcp->data;
    HdcTCPBase *thisClass = (HdcTCPBase *)hSession->classModule;
    HdcSessionBase *hSessionBase = (HdcSessionBase *)thisClass->clsMainBase;
    CALLSTAT_GUARD(hSession->childLoopStatus, tcp->loop, "HdcTCPBase::ReadStream");
    bool ret = false;
    std::string sessionIdMaskStr = Hdc::MaskSessionIdToString(hSession->sessionId);
    while (true) {
        if (nread < 0) {
            break;
        }
#ifdef HDC_SUPPORT_ENCRYPT_TCP
        if (hSessionBase->FetchIOBuf(hSession, hSession->ioBuf, nread, hSession->sslHandshake) < 0) {
            WRITE_LOG(LOG_FATAL, "ReadStream FetchIOBuf error nread:%zd, sid:%s", nread, sessionIdMaskStr.c_str());
            break;
        }
#else
        if (hSessionBase->FetchIOBuf(hSession, hSession->ioBuf, nread) < 0) {
            WRITE_LOG(LOG_FATAL, "ReadStream FetchIOBuf error nread:%zd, sid:%s", nread, sessionIdMaskStr.c_str());
            break;
        }
#endif
        ret = true;
        break;
    }
    if (!ret) {
        char buffer[BUF_SIZE_DEFAULT] = { 0 };
        if (nread < 0) {
            uv_strerror_r(static_cast<int>(nread), buffer, BUF_SIZE_DEFAULT);
            WRITE_LOG(LOG_INFO, "HdcTCPBase::ReadStream < 0 %s sid:%s", buffer, sessionIdMaskStr.c_str());
        }
#ifdef HDC_HOST
        hSession->isRunningOk = false;
        {
            std::lock_guard<std::mutex> lock(hSession->faultInfoMutex);
            hSession->faultInfo = (nread < 0) ? std::string(buffer) : "package parse error";
        }
#endif
        // The first time is closed first, prevent the write function from continuing to write
        Base::TryCloseHandle(reinterpret_cast<uv_handle_t *>(tcp));
        hSessionBase->FreeSession(hSession->sessionId);
    }
}

bool HdcTCPBase::WriteUvSslFd(uv_tcp_t *tcp, uint8_t *buf, int size, int &cnt)
{
    if (!tcp || !buf || size < 0 ||cnt < 0) {
        WRITE_LOG(LOG_FATAL, "WriteUvSslFd error, input parameter abnormal.");
        return false;
    }
#ifdef HDC_SUPPORT_ENCRYPT_TCP
    HSession hSession = reinterpret_cast<HSession>(tcp->data);
    if (!hSession) {
        WRITE_LOG(LOG_FATAL, "WriteUvSslFd error, hSession is null.");
        return false;
    }

    if (!hSession->sslHandshake) { // plaintext tcp transfer.
        return true;
    }

    HdcSSLBase *hssl = static_cast<HdcSSLBase *>(hSession->classSSL);
    if (!hssl) {
        WRITE_LOG(LOG_FATAL, "WriteUvSslFd error, hssl is null.");
        return false;
    }

    cnt = hssl->Encrypt(size, buf);
    if (cnt < 0) {
        WRITE_LOG(LOG_FATAL, "WriteSSL error, cnt:%d", cnt);
        return false;
    }
#endif
    return true;
}

int HdcTCPBase::WriteUvTcpFd(uv_tcp_t *tcp, uint8_t *buf, int size)
{
    std::lock_guard<std::mutex> lock(writeTCPMutex);
    uint8_t *data = buf;
    int cnt = size;
    if (!WriteUvSslFd(tcp, buf, size, cnt)) {
        delete[] buf;
        return ERR_GENERIC;
    }
    uv_os_fd_t uvfd;
    uv_fileno(reinterpret_cast<uv_handle_t*>(tcp), &uvfd);
#ifdef _WIN32
    int fd = (uv_os_sock_t)uvfd;
#else
    int fd = reinterpret_cast<int>(uvfd);
#endif
    constexpr int intrmax = 60000;
    int intrcnt = 0;
    while (cnt > 0) {
#ifdef HDC_EMULATOR
        int rc = write(fd, reinterpret_cast<const char*>(data), cnt);
#else
        int rc = send(fd, reinterpret_cast<const char*>(data), cnt, 0);
#endif
        if (rc < 0) {
#ifdef _WIN32
            int err = WSAGetLastError();
            if (err == WSAEINTR || err == WSAEWOULDBLOCK) {
#else
            int err = errno;
            if (err == EINTR || err == EAGAIN) {
#endif
                if (++intrcnt > intrmax) {
                    WRITE_LOG(LOG_WARN, "WriteUvTcpFd fd:%d send interrupt err:%d", fd, err);
                    intrcnt = 0;
                }
                std::this_thread::yield();
                continue;
            } else {
                WRITE_LOG(LOG_FATAL, "WriteUvTcpFd fd:%d send rc:%d err:%d", fd, rc, err);
                cnt = ERR_GENERIC;
                break;
            }
        }
        data += rc;
        cnt -= rc;
    }
    delete[] buf;
    return cnt == 0 ? size : cnt;
}
}  // namespace Hdc
