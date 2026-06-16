package com.linkhub.app

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.os.Build

class LinkHubApp : Application() {
    companion object {
        const val CHANNEL_LISTENER = "linkhub_listener"
        const val CHANNEL_TRANSFERS = "linkhub_transfers"
        lateinit var instance: LinkHubApp
    }

    override fun onCreate() {
        super.onCreate()
        instance = this
        createNotificationChannels()
    }

    private fun createNotificationChannels() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val nm = getSystemService(NotificationManager::class.java)
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_LISTENER, "LinkHub Listener",
                    NotificationManager.IMPORTANCE_LOW).apply {
                    description = "Shown when LinkHub is listening for connections"
                })
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_TRANSFERS, "Transfers",
                    NotificationManager.IMPORTANCE_DEFAULT).apply {
                    description = "File and text transfer progress"
                })
        }
    }
}
