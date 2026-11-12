package com.rarebox.wear.core

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * 通过 mDNS 发现本机/局域网上的 ADB 无线调试服务。
 *
 * - `_adb-tls-pairing._tcp`：配对码弹窗打开时广播，用于 `adb pair`；
 * - `_adb-tls-connect._tcp`：无线调试开启期间广播，用于 `adb connect`。
 *
 * 手表自己没有键盘时，自动拿到端口可以少输一段地址。
 */
class AdbDiscovery(context: Context) {

    data class Service(val name: String, val host: String, val port: Int, val kind: String) {
        val address: String get() = "$host:$port"
    }

    private val nsd = context.applicationContext.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val listeners = mutableListOf<NsdManager.DiscoveryListener>()
    private val resolved = linkedMapOf<String, Service>()

    private val _services = MutableStateFlow<List<Service>>(emptyList())
    val services = _services.asStateFlow()

    fun start() {
        if (listeners.isNotEmpty()) return
        discover("_adb-tls-pairing._tcp", "pairing")
        discover("_adb-tls-connect._tcp", "connect")
    }

    fun stop() {
        listeners.forEach { listener ->
            runCatching { nsd.stopServiceDiscovery(listener) }
                .onFailure { Log.w(TAG, "stop discovery failed", it) }
        }
        listeners.clear()
        resolved.clear()
        _services.value = emptyList()
    }

    private fun discover(type: String, kind: String) {
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) = Unit

            override fun onServiceFound(info: NsdServiceInfo) {
                runCatching {
                    nsd.resolveService(info, object : NsdManager.ResolveListener {
                        override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                            Log.d(TAG, "resolve ${serviceInfo.serviceName} failed: $errorCode")
                        }

                        override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                            val host = serviceInfo.host?.hostAddress ?: return
                            val service = Service(serviceInfo.serviceName, host, serviceInfo.port, kind)
                            resolved[service.name] = service
                            _services.value = resolved.values.toList()
                        }
                    })
                }.onFailure { Log.w(TAG, "resolve request failed", it) }
            }

            override fun onServiceLost(info: NsdServiceInfo) {
                resolved.remove(info.serviceName)
                _services.value = resolved.values.toList()
            }

            override fun onDiscoveryStopped(serviceType: String) = Unit

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.w(TAG, "start discovery $serviceType failed: $errorCode")
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) = Unit
        }
        listeners += listener
        runCatching { nsd.discoverServices(type, NsdManager.PROTOCOL_DNS_SD, listener) }
            .onFailure { Log.w(TAG, "discoverServices($type) failed", it) }
    }

    companion object {
        private const val TAG = "AdbDiscovery"
    }
}
