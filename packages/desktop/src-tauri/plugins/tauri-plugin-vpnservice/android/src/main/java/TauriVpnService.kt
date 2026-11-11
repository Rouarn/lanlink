package com.plugin.vpnservice

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.ParcelFileDescriptor
import android.util.Log
import androidx.core.app.NotificationCompat
import app.tauri.plugin.JSObject
import android.system.OsConstants.AF_INET6

/**
 * LanLink 的虚拟网卡服务。
 *
 * 建卡后把 fd 同时放进两个通道：
 * - `triggerCallback("vpn_service_start", {fd})`（保留给 WebView 侧）
 * - [pendingFd]，由 Rust 侧通过 `take_tun_fd` 命令取走（本应用的主路径）
 *
 * fd 是进程级资源，Kotlin 与 libeasytier.so 在同一个进程里，直接传整数即可。
 */
class TauriVpnService : VpnService() {
    companion object {
        @JvmField var triggerCallback: (String, JSObject) -> Unit = { _, _ -> }
        @JvmField var self: TauriVpnService? = null
        @JvmField var pendingFd: Int = -1
        @JvmField var ipv4Addr: String? = null
        @JvmField var ipv4Addrs: Array<String> = emptyArray()
        @JvmField var routes: Array<String> = emptyArray()
        @JvmField var dns: String? = null

        const val ACTION_STOP = "com.plugin.vpnservice.STOP"
        const val IPV4_ADDR = "IPV4_ADDR"
        const val IPV4_ADDRS = "IPV4_ADDRS"
        const val ROUTES = "ROUTES"
        const val DNS = "DNS"
        const val DISALLOWED_APPLICATIONS = "DISALLOWED_APPLICATIONS"
        const val MTU = "MTU"

        private const val NOTIFICATION_CHANNEL_ID = "lanlink_vpn"
        private const val NOTIFICATION_ID = 1356
        private const val TAG = "LanLinkVpn"
    }

    private var vpnInterface: ParcelFileDescriptor? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            disconnect()
            stopForeground(STOP_FOREGROUND_REMOVE)
            self = null
            stopSelf(startId)
            return START_NOT_STICKY
        }

        // 先进入前台再处理任何分支：startForegroundService() 有硬性时限，
        // 在 startForeground() 之前 stopSelf() 会被系统判定为 FGS 超时并直接杀掉进程。
        return try {
            startVpnForegroundService()

            val args = intent?.extras
            val addrs = getIpv4Addrs(args)
            // 进程被系统回收后重建时没有 Intent，拿不到地址就不该勉强建卡。
            if (addrs.isEmpty()) {
                Log.w(TAG, "没有可用的 IPv4 地址，放弃建卡")
                disconnect()
                stopForeground(STOP_FOREGROUND_REMOVE)
                self = null
                stopSelf(startId)
                return START_NOT_STICKY
            }

            ipv4Addrs = addrs
            ipv4Addr = addrs.first()
            routes = args?.getStringArray(ROUTES) ?: emptyArray()
            dns = args?.getString(DNS)

            val fd = createVpnInterface(args)
            vpnInterface = fd
            pendingFd = fd.fd
            Log.i(TAG, "establish 成功，tun fd=${fd.fd}，地址=${addrs.joinToString(",")}")

            val event = JSObject()
            event.put("fd", fd.fd)
            triggerCallback("vpn_service_start", event)
            START_STICKY
        } catch (err: Throwable) {
            // 建卡失败（多为权限被回收或地址非法）：把原因交给 Rust 侧展示，别让进程崩掉。
            Log.e(TAG, "startVpn 失败", err)
            val event = JSObject()
            event.put("message", err.message ?: err.toString())
            triggerCallback("vpn_service_error", event)
            disconnect()
            stopForeground(STOP_FOREGROUND_REMOVE)
            self = null
            stopSelf(startId)
            START_NOT_STICKY
        }
    }

    override fun onCreate() {
        super.onCreate()
        self = this
    }

    override fun onDestroy() {
        disconnect()
        stopForeground(STOP_FOREGROUND_REMOVE)
        self = null
        super.onDestroy()
    }

    override fun onRevoke() {
        disconnect()
        stopForeground(STOP_FOREGROUND_REMOVE)
        self = null
        super.onRevoke()
    }

    private fun disconnect() {
        pendingFd = -1
        vpnInterface?.let {
            runCatching { it.close() }
            triggerCallback("vpn_service_stop", JSObject())
        }
        vpnInterface = null
        clearStatus()
    }

    private fun clearStatus() {
        ipv4Addr = null
        ipv4Addrs = emptyArray()
        routes = emptyArray()
        dns = null
    }

    private fun startVpnForegroundService() {
        createNotificationChannel()

        val launchIntent = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        }
        val contentIntent = launchIntent?.let {
            PendingIntent.getActivity(
                this,
                0,
                it,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }
        val notification = NotificationCompat.Builder(this, NOTIFICATION_CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_menu_manage)
            .setContentTitle("LanLink 正在运行")
            .setContentText("虚拟网卡已建立，正在与其他设备组网")
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .apply { contentIntent?.let(::setContentIntent) }
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                NOTIFICATION_CHANNEL_ID,
                "LanLink 虚拟网卡",
                NotificationManager.IMPORTANCE_LOW,
            )
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
    }

    private fun getIpv4Addrs(args: Bundle?): Array<String> {
        val addrs = args
            ?.getStringArray(IPV4_ADDRS)
            ?.filter { it.isNotBlank() }
            ?.toTypedArray()
            ?: emptyArray()
        if (addrs.isNotEmpty()) {
            return addrs
        }
        return listOfNotNull(args?.getString(IPV4_ADDR)?.takeIf { it.isNotBlank() }).toTypedArray()
    }

    private fun createVpnInterface(args: Bundle?): ParcelFileDescriptor {
        val builder = Builder()
            .setSession("LanLink")
            .setBlocking(false)

        val mtu = args?.getInt(MTU) ?: 1500
        val addrs = getIpv4Addrs(args)
        val dns = args?.getString(DNS)
        val routes = args?.getStringArray(ROUTES) ?: emptyArray()
        val disallowedApplications = args?.getStringArray(DISALLOWED_APPLICATIONS) ?: emptyArray()

        for (addr in addrs) {
            val parts = addr.split("/")
            require(parts.size == 2) { "非法地址：$addr" }
            builder.addAddress(parts[0], parts[1].toInt())
        }
        builder.allowFamily(AF_INET6)
        builder.setMtu(mtu)
        dns?.let { builder.addDnsServer(it) }
        for (route in routes) {
            val parts = route.split("/")
            require(parts.size == 2) { "非法路由：$route" }
            builder.addRoute(parts[0], parts[1].toInt())
        }
        for (app in disallowedApplications) {
            builder.addDisallowedApplication(app)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }

        return builder.establish() ?: throw IllegalStateException("创建虚拟网卡失败（establish 返回空）")
    }
}
