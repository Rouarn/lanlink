package com.plugin.vpnservice

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Build
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject

@InvokeArg
class StartVpnArgs {
    var ipv4Addr: String? = null
    // 数组字段一律可空：Jackson 会把 JSON null 反射写进来，
    // 非空声明挡不住 null，使用处再 `?: emptyArray()` 兜底。
    var ipv4Addrs: Array<String>? = null
    var routes: Array<String>? = null
    var dns: String? = null
    var disallowedApplications: Array<String>? = null
    var mtu: Int? = null
}

/**
 * 把系统 VpnService 暴露给 Rust 侧：授权 → 建卡 → 交出 fd → 停止。
 *
 * 注意：Rust 侧通过 `take_tun_fd` 主动取 fd，而不是依赖 WebView 事件，
 * 这样即使前端还没加载完也能正常工作。
 */
@TauriPlugin
class VpnServicePlugin(private val activity: Activity) : Plugin(activity) {

    override fun load(webView: android.webkit.WebView) {
        TauriVpnService.triggerCallback = { event, data ->
            trigger(event, data)
        }
    }

    @Command
    fun prepareVpn(invoke: Invoke) {
        activity.runOnUiThread {
            val intent = VpnService.prepare(activity)
            if (intent != null) {
                startActivityForResult(invoke, intent, "onPrepareVpnResult")
                return@runOnUiThread
            }
            val ret = JSObject()
            ret.put("granted", true)
            invoke.resolve(ret)
        }
    }

    @ActivityCallback
    fun onPrepareVpnResult(invoke: Invoke, result: ActivityResult) {
        val ret = JSObject()
        ret.put("granted", result.resultCode == Activity.RESULT_OK)
        invoke.resolve(ret)
    }

    @Command
    fun startVpn(invoke: Invoke) {
        val args = invoke.parseArgs(StartVpnArgs::class.java)
        activity.runOnUiThread {
            TauriVpnService.self?.onRevoke()

            val prepareIntent = VpnService.prepare(activity)
            val ret = JSObject()
            if (prepareIntent != null) {
                ret.put("errorMsg", "need_prepare")
                invoke.resolve(ret)
                return@runOnUiThread
            }

            val intent = Intent(activity, TauriVpnService::class.java)
            intent.putExtra(TauriVpnService.IPV4_ADDR, args.ipv4Addr)
            intent.putExtra(TauriVpnService.IPV4_ADDRS, args.ipv4Addrs ?: emptyArray())
            intent.putExtra(TauriVpnService.ROUTES, args.routes ?: emptyArray())
            intent.putExtra(TauriVpnService.DNS, args.dns)
            intent.putExtra(TauriVpnService.DISALLOWED_APPLICATIONS, args.disallowedApplications ?: emptyArray())
            intent.putExtra(TauriVpnService.MTU, args.mtu)

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                activity.startForegroundService(intent)
            } else {
                activity.startService(intent)
            }
            invoke.resolve(ret)
        }
    }

    @Command
    fun stopVpn(invoke: Invoke) {
        activity.runOnUiThread {
            val service = TauriVpnService.self
            if (service != null) {
                val intent = Intent(activity, TauriVpnService::class.java)
                intent.action = TauriVpnService.ACTION_STOP
                activity.startService(intent)
            }
            invoke.resolve(JSObject())
        }
    }

    @Command
    fun getVpnStatus(invoke: Invoke) {
        val ret = JSObject()
        val service = TauriVpnService.self
        ret.put("running", service != null && TauriVpnService.ipv4Addr != null)
        ret.put("ipv4Addr", TauriVpnService.ipv4Addr ?: JSONObject.NULL)
        ret.put("ipv4Addrs", TauriVpnService.ipv4Addrs)
        ret.put("routes", TauriVpnService.routes)
        ret.put("dns", TauriVpnService.dns ?: JSONObject.NULL)
        invoke.resolve(ret)
    }

    /** 取走建卡得到的 tun fd（取一次即清空），供 Rust 侧交给 EasyTier。 */
    @Command
    fun takeTunFd(invoke: Invoke) {
        val ret = JSObject()
        ret.put("fd", TauriVpnService.pendingFd)
        TauriVpnService.pendingFd = -1
        invoke.resolve(ret)
    }
}
