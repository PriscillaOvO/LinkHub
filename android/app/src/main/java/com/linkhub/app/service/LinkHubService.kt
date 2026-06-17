package com.linkhub.app.service

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat
import com.google.gson.Gson
import com.linkhub.app.LinkHubApp
import com.linkhub.app.MainActivity
import com.linkhub.app.bridge.RustBridge
import com.linkhub.app.ui.defaultReceiveDir
import com.linkhub.app.ui.ensureRustTrustStore
import com.linkhub.app.ui.handleReceivedFile
import com.linkhub.app.ui.loadServiceStatus
import com.linkhub.app.ui.loadIdentity
import com.linkhub.app.ui.loadIdentityJson
import com.linkhub.app.ui.saveServiceStatus
import com.linkhub.app.ui.startAndroidMdnsAdvertise
import com.linkhub.app.ui.stopAndroidMdnsAdvertise

class LinkHubService : Service() {
    companion object {
        // Written on the service/monitor threads (onStartCommand, onDestroy, the
        // listener-monitor daemon) and read on the Compose UI thread as the sole
        // liveness signal — must be @Volatile so the UI never observes a stale
        // value (a stale read could re-disable 启动监听 with nothing listening).
        @Volatile
        var isRunning = false
            private set
    }

    private val NOTIFICATION_ID = 1001
    private val gson = Gson()
    @Volatile
    private var monitorActive = false
    private var monitorThread: Thread? = null

    data class ListenerResult(
        val running: Boolean = false,
        val detail: String = "",
        val error: String = ""
    )

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val addr = intent?.getStringExtra("listen_addr") ?: "0.0.0.0:8787"
        val receiveDir = intent?.getStringExtra("receive_dir") ?: defaultReceiveDir(this)

        startForeground(NOTIFICATION_ID, buildNotification("LinkHub listener: $addr"))

        val appContext = applicationContext
        RustBridge.onFileReceivedListener = { peerDeviceId, peerDeviceName, fileName, filePath, sizeBytes ->
            try {
                handleReceivedFile(appContext, peerDeviceId, peerDeviceName, fileName, filePath, sizeBytes)
            } catch (_: Exception) {
            }
        }

        saveServiceStatus(
            this,
            running = false,
            listenAddr = addr,
            receiveDir = receiveDir,
            detail = "starting listener"
        )
        try {
            val identityJson = loadIdentityJson(this)
                ?: throw IllegalStateException("No identity configured. Generate one in Pair tab.")
            val identity = loadIdentity(this)
                ?: throw IllegalStateException("Invalid identity data. Regenerate identity in Pair tab.")
            val trustStorePath = ensureRustTrustStore(this)
            val resultJson = RustBridge.startListener(identityJson, addr, trustStorePath, receiveDir)
            val result = try {
                gson.fromJson(resultJson, ListenerResult::class.java)
            } catch (_: Exception) {
                ListenerResult(error = resultJson)
            }

            if (result.error.isNotBlank()) {
                isRunning = false
                saveServiceStatus(
                    this,
                    running = false,
                    listenAddr = addr,
                    receiveDir = receiveDir,
                    error = result.error
                )
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            } else {
                isRunning = result.running
                var mdnsName = ""
                if (result.running) {
                    val port = addr.substringAfterLast(':', "8787").toIntOrNull() ?: 8787
                    mdnsName = startAndroidMdnsAdvertise(this, identity, port)
                    startListenerMonitor(addr, receiveDir, mdnsName)
                }
                saveServiceStatus(
                    this,
                    running = result.running,
                    listenAddr = addr,
                    receiveDir = receiveDir,
                    detail = result.detail.ifBlank { "listener started" },
                    mdnsServiceName = mdnsName
                )
            }
        } catch (e: Exception) {
            isRunning = false
            saveServiceStatus(
                this,
                running = false,
                listenAddr = addr,
                receiveDir = receiveDir,
                error = e.message ?: e::class.java.simpleName
            )
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }

        return START_STICKY
    }

    override fun onDestroy() {
        monitorActive = false
        RustBridge.onFileReceivedListener = null
        try {
            RustBridge.stopListener()
        } catch (_: Exception) {}
        try {
            stopAndroidMdnsAdvertise(this)
        } catch (_: Exception) {}
        val previousStatus = loadServiceStatus(this)
        if (previousStatus.error.isBlank()) {
            saveServiceStatus(this, running = false, detail = "listener stopped")
        }
        isRunning = false
        super.onDestroy()
    }

    private fun startListenerMonitor(listenAddr: String, receiveDir: String, mdnsServiceName: String) {
        monitorActive = false
        monitorThread?.interrupt()
        monitorActive = true
        monitorThread = Thread {
            while (monitorActive) {
                try {
                    Thread.sleep(1_000)
                    val result = try {
                        gson.fromJson(RustBridge.listenerStatus(), ListenerResult::class.java)
                    } catch (e: Exception) {
                        ListenerResult(error = e.message ?: e::class.java.simpleName)
                    }
                    if (!result.running) {
                        monitorActive = false
                        isRunning = false
                        saveServiceStatus(
                            this,
                            running = false,
                            listenAddr = listenAddr,
                            receiveDir = receiveDir,
                            detail = result.detail.ifBlank { "listener stopped" },
                            error = result.error,
                            mdnsServiceName = mdnsServiceName
                        )
                        try {
                            stopAndroidMdnsAdvertise(this)
                        } catch (_: Exception) {}
                        stopSelf()
                    } else {
                        saveServiceStatus(
                            this,
                            running = true,
                            listenAddr = listenAddr,
                            receiveDir = receiveDir,
                            detail = result.detail.ifBlank { "listener running" },
                            error = "",
                            mdnsServiceName = mdnsServiceName
                        )
                    }
                } catch (_: InterruptedException) {
                    monitorActive = false
                }
            }
        }.apply {
            name = "LinkHubListenerMonitor"
            isDaemon = true
            start()
        }
    }

    private fun buildNotification(text: String): Notification {
        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val pendingIntent = PendingIntent.getActivity(
            this, 0, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(this, LinkHubApp.CHANNEL_LISTENER)
            .setContentTitle("LinkHub")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }
}
