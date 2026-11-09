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

#include "daemon_tcp.h"

#include <cstdlib>

#include "arpa/inet.h"
#include "netinet/in.h"
#include "new"
#include "sys/socket.h"
#include "unistd.h"

#include "common.h"
#include "session.h"
#include "system_depend.h"

namespace Hdc {
HdcDaemonTCP::HdcDaemonTCP(const bool serverOrDaemonIn, void *ptrMainBase)
    : HdcTCPBase(serverOrDaemonIn, ptrMainBase)
{
    // If the listening value for the property setting is obtained, it will be 0 randomly assigned.
    string strTCPPort;
    SystemDepend::GetDevItem("persist.hdc.port", strTCPPort);
    tcpListenPort = atoi(strTCPPort.c_str());
    if (tcpListenPort <= 0) {
        WRITE_LOG(LOG_WARN, "persist.hdc.port is invalid, set default to 0");
        tcpListenPort = 0;
    }
}

HdcDaemonTCP::~HdcDaemonTCP()
{
}

void HdcDaemonTCP::Stop()
{
    Base::TryCloseHandle((const uv_handle_t *)&servUDP);
    Base::TryCloseHandle((const uv_handle_t *)&servTCP);
    WRITE_LOG(LOG_INFO, "Stop tcpListenPort:%u", tcpListenPort);
}

void HdcDaemonTCP::BuildUDPReplyPayload()
{
    // Wire format is unchanged: HANDSHAKE_MESSAGE + "-" + <port>.
    udpReplyPayload = HANDSHAKE_MESSAGE + "-" + std::to_string(tcpListenPort);
}

void HdcDaemonTCP::TransmitConfig(const sockaddr *addrSrc, uv_udp_t *handle)
{
    if (addrSrc == nullptr) {
        WRITE_LOG(LOG_WARN, "TransmitConfig addrSrc is null");
        return;
    }
    if (handle == nullptr) {
        WRITE_LOG(LOG_WARN, "TransmitConfig handle is null");
        return;
    }
    if (addrSrc->sa_family != AF_INET) {
        // Reading an AF_INET6 address as sockaddr_in would pick up garbage.
        WRITE_LOG(LOG_WARN, "TransmitConfig unsupported sa_family:%d", static_cast<int>(addrSrc->sa_family));
        return;
    }
    if (udpReplyPayload.empty()) {
        WRITE_LOG(LOG_FATAL, "TransmitConfig udpReplyPayload is empty");
        return;
    }
    const struct sockaddr_in *srcIn = reinterpret_cast<const struct sockaddr_in *>(addrSrc);
    struct sockaddr_in addrDst = {};
    addrDst.sin_family = AF_INET;
    addrDst.sin_port = htons(DEFAULT_PORT);
    addrDst.sin_addr = srcIn->sin_addr;

    uv_buf_t sndbuf = uv_buf_init(const_cast<char *>(udpReplyPayload.data()),
                                  static_cast<size_t>(udpReplyPayload.size()));
    uv_udp_send_t *req = new (std::nothrow) uv_udp_send_t();
    if (req == nullptr) {
        WRITE_LOG(LOG_FATAL, "TransmitConfig req is null, payload size:%zu", udpReplyPayload.size());
        return;
    }
    // uv_udp_send may defer the actual sendmsg, so the payload must not live on the stack.
    int rc = uv_udp_send(req, handle, &sndbuf, 1,
                         reinterpret_cast<const struct sockaddr *>(&addrDst), SendUDPFinish);
    if (rc < 0) {
        WRITE_LOG(LOG_WARN, "TransmitConfig uv_udp_send failed rc:%d", rc);
        delete req;  // the callback is not invoked when uv_udp_send fails
    }
}

void HdcDaemonTCP::AcceptClient(uv_stream_t *server, int /* status */)
{
    uv_loop_t *ptrLoop = server->loop;
    uv_tcp_t *pServTCP = (uv_tcp_t *)server;
    HdcDaemonTCP *thisClass = (HdcDaemonTCP *)pServTCP->data;
    HdcSessionBase *ptrConnect = reinterpret_cast<HdcSessionBase *>(thisClass->clsMainBase);
    HdcSessionBase *daemon = reinterpret_cast<HdcSessionBase *>(thisClass->clsMainBase);
    CALLSTAT_GUARD(daemon->loopMainStatus, server->loop, "HdcDaemonTCP::AcceptClient");
    const uint16_t maxWaitTime = UV_DEFAULT_INTERVAL;
    auto ctrl = daemon->BuildCtrlString(SP_START_SESSION, 0, nullptr, 0);
    HSession hSession = ptrConnect->MallocSession(false, CONN_TCP, thisClass);
    if (!hSession) {
        WRITE_LOG(LOG_FATAL, "malloc tcp session failed");
        return;
    }
    if (uv_accept(server, (uv_stream_t *)&hSession->hWorkTCP) < 0) {
        WRITE_LOG(LOG_FATAL, "uv_accept error sessionId:%s",
            Hdc::MaskSessionIdToString(hSession->sessionId).c_str());
        goto Finish;
    }
    if ((hSession->fdChildWorkTCP = Base::DuplicateUvSocket(&hSession->hWorkTCP)) < 0) {
        WRITE_LOG(LOG_FATAL, "AcceptClient error fdChildWorkTCP:%d,errno:%d", hSession->fdChildWorkTCP, errno);
        goto Finish;
    }
    Base::TryCloseHandle((uv_handle_t *)&hSession->hWorkTCP);
    Base::StartWorkThread(ptrLoop, ptrConnect->SessionWorkThread, Base::FinishWorkThread, hSession);
    // wait for thread up
    while (hSession->childLoop.active_handles == 0) {
        usleep(maxWaitTime);
    }
    Base::SendToPollFd(hSession->ctrlFd[STREAM_MAIN], ctrl.data(), ctrl.size());
    return;
Finish:
    ptrConnect->FreeSession(hSession->sessionId);
}

void HdcDaemonTCP::RecvUDPEntry(const sockaddr *addrSrc, uv_udp_t *handle, const uv_buf_t * /* rcvbuf */,
                                ssize_t nread)
{
    // A discovery request is exactly HANDSHAKE_MESSAGE with no suffix. Our own reply
    // "OHOS HDC-<port>" is longer and is dropped here -- answering a reply is what makes
    // two endpoints echo each other forever. nread is the payload length RecvUDP already
    // knows (and only calls here once nread > 0), so the buffer is not rescanned for it.
    size_t packetLen = static_cast<size_t>(nread);
    if (packetLen != HANDSHAKE_MESSAGE.size()) {
        WRITE_LOG(LOG_DEBUG, "RecvUDPEntry drop packet len:%zu", packetLen);
        return;
    }
    TransmitConfig(addrSrc, handle);
}

int HdcDaemonTCP::SetUDPListen()
{
    struct sockaddr_in addr;
    HdcSessionBase *ptrConnect = (HdcSessionBase *)clsMainBase;
    // udp broadcast
    // Keep plain uv_udp_init: AllocStreamUDP hands out a single small buffer, which
    // the recvmmsg path cannot work with (it splits the buffer into 64KB chunks).
    servUDP.data = this;
    if (uv_udp_init(&ptrConnect->loopMain, &servUDP) != 0) {
        WRITE_LOG(LOG_FATAL, "uv_udp_init failed");
        return ERR_API_FAIL;
    }
    if (uv_ip4_addr("0.0.0.0", DEFAULT_PORT, &addr) != 0) {
        WRITE_LOG(LOG_FATAL, "uv_ip4_addr failed");
        return ERR_API_FAIL;
    }
    if (uv_udp_bind(&servUDP, (const struct sockaddr *)&addr, UV_UDP_REUSEADDR) != 0) {
        WRITE_LOG(LOG_FATAL, "uv_udp_bind failed");
        return ERR_API_FAIL;
    }
    if (uv_udp_recv_start(&servUDP, AllocStreamUDP, RecvUDP) != 0) {
        WRITE_LOG(LOG_FATAL, "uv_udp_recv_start failed");
        return ERR_API_FAIL;
    }
    return RET_SUCCESS;
}

// Set the daemon-side TCP listening
int HdcDaemonTCP::SetTCPListen()
{
    // tcp listen
    HdcSessionBase *ptrConnect = (HdcSessionBase *)clsMainBase;
    servTCP.data = this;
    struct sockaddr_in addr = {};
    int namelen;
    const int DEFAULT_BACKLOG = 128;

    uv_tcp_init(&ptrConnect->loopMain, &servTCP);
    uv_ip4_addr("0.0.0.0", tcpListenPort, &addr);  // tcpListenPort == 0
    uv_tcp_bind(&servTCP, (const struct sockaddr *)&addr, 0);
    if (uv_listen((uv_stream_t *)&servTCP, DEFAULT_BACKLOG, (uv_connection_cb)AcceptClient)) {
        return ERR_API_FAIL;
    }
    // Get listen port
    (void)memset_s(&addr, sizeof(sockaddr_in), 0, sizeof(sockaddr_in));
    namelen = sizeof(addr);
    if (uv_tcp_getsockname(&servTCP, (sockaddr *)&addr, &namelen)) {
        return ERR_API_FAIL;
    }
    tcpListenPort = ntohs(addr.sin_port);
    SystemDepend::SetDevItem("persist.hdc.port", std::to_string(tcpListenPort).c_str());
    // The port is final here and the event loop is not running yet, so the cached
    // reply payload can be built once and stays stable for every later send.
    BuildUDPReplyPayload();
    return RET_SUCCESS;
}

int HdcDaemonTCP::Initial()
{
    WRITE_LOG(LOG_INFO, "HdcDaemonTCP init");
    if (SetUDPListen() != RET_SUCCESS) {
        WRITE_LOG(LOG_FATAL, "UDP listen failed");
        return ERR_GENERIC;
    }
    if (SetTCPListen() != RET_SUCCESS) {
        WRITE_LOG(LOG_FATAL, "TCP listen failed");
        return ERR_GENERIC;
    }
#ifndef UNIT_TEST
    WRITE_LOG(LOG_INFO, "TCP listen on port:[%d]", tcpListenPort);
#endif
    return RET_SUCCESS;
}
}  // namespace Hdc
