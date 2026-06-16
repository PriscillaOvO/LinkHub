package com.linkhub.app.ui

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.linkhub.app.LinkHubApp
import kotlin.math.absoluteValue

private const val TRANSFER_NOTIFICATION_GROUP = "linkhub_transfers_group"

fun showTransferNotification(
    ctx: Context,
    peer: TrustedPeer,
    kind: String,
    title: String,
    detail: String,
    inProgress: Boolean = false
) {
    if (!canPostNotifications(ctx)) return

    val notificationId = notificationIdFor(peer, kind)
    val notification = NotificationCompat.Builder(ctx, LinkHubApp.CHANNEL_TRANSFERS)
        .setSmallIcon(android.R.drawable.stat_sys_upload)
        .setContentTitle(title)
        .setContentText(detail)
        .setStyle(NotificationCompat.BigTextStyle().bigText(detail))
        .setGroup(TRANSFER_NOTIFICATION_GROUP)
        .setOngoing(inProgress)
        .setOnlyAlertOnce(inProgress)
        .setAutoCancel(!inProgress)
        .setPriority(NotificationCompat.PRIORITY_DEFAULT)
        .build()

    NotificationManagerCompat.from(ctx).notify(notificationId, notification)
}

fun showReceivedFileNotification(
    ctx: Context,
    peerKey: String,
    title: String,
    detail: String
) {
    if (!canPostNotifications(ctx)) return

    val notificationId = "linkhub:received:$peerKey".hashCode().absoluteValue
    val notification = NotificationCompat.Builder(ctx, LinkHubApp.CHANNEL_TRANSFERS)
        .setSmallIcon(android.R.drawable.stat_sys_download_done)
        .setContentTitle(title)
        .setContentText(detail)
        .setStyle(NotificationCompat.BigTextStyle().bigText(detail))
        .setGroup(TRANSFER_NOTIFICATION_GROUP)
        .setAutoCancel(true)
        .setPriority(NotificationCompat.PRIORITY_DEFAULT)
        .build()

    NotificationManagerCompat.from(ctx).notify(notificationId, notification)
}

private fun canPostNotifications(ctx: Context): Boolean {
    return Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
}

private fun notificationIdFor(peer: TrustedPeer, kind: String): Int {
    return "linkhub:${peer.deviceId}:$kind".hashCode().absoluteValue
}
