package com.linkhub.app

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.content.pm.PackageManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.compose.foundation.layout.*
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.linkhub.app.bridge.RustBridge
import com.linkhub.app.ui.DevicesScreen
import com.linkhub.app.ui.HistoryScreen
import com.linkhub.app.ui.PairScreen
import com.linkhub.app.ui.SendScreen
import com.linkhub.app.ui.ServiceScreen
import com.linkhub.app.ui.saveTrustedPeerFromIncoming
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

class MainActivity : ComponentActivity() {
    private val pendingShareUris = mutableStateOf<List<Uri>>(emptyList())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestNotificationPermissionIfNeeded()
        pendingShareUris.value = shareUrisFromIntent(intent)
        setContent {
            LinkHubTheme {
                LinkHubMain(
                    sharedUris = pendingShareUris.value,
                    onSharedUrisConsumed = { pendingShareUris.value = emptyList() }
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        pendingShareUris.value = shareUrisFromIntent(intent)
    }

    private fun requestNotificationPermissionIfNeeded() {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
                PackageManager.PERMISSION_GRANTED
        ) {
            ActivityCompat.requestPermissions(
                this,
                arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                100
            )
        }
    }
}

enum class Tab { Pair, Devices, Send, History, Service }

private val incomingPeerRequestIds = AtomicLong(0)

private data class PendingIncomingPeer(
    val requestId: Long,
    val peer: RustBridge.IncomingPeer,
    val complete: (Boolean) -> Unit
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkHubMain(
    sharedUris: List<Uri> = emptyList(),
    onSharedUrisConsumed: () -> Unit = {}
) {
    val ctx = LocalContext.current
    val appCtx = ctx.applicationContext
    val mainHandler = remember { Handler(Looper.getMainLooper()) }
    var currentTab by remember { mutableStateOf(Tab.Pair) }
    var pendingIncomingPeer by remember { mutableStateOf<PendingIncomingPeer?>(null) }

    LaunchedEffect(sharedUris) {
        if (sharedUris.isNotEmpty()) {
            currentTab = Tab.Send
        }
    }

    DisposableEffect(appCtx) {
        RustBridge.onIncomingPeerListener = { peer ->
            val latch = CountDownLatch(1)
            val accepted = AtomicBoolean(false)
            val resolved = AtomicBoolean(false)
            val requestId = incomingPeerRequestIds.incrementAndGet()
            val complete: (Boolean) -> Unit = { allow ->
                if (resolved.compareAndSet(false, true)) {
                    if (allow) {
                        try {
                            saveTrustedPeerFromIncoming(
                                appCtx,
                                peer.deviceId,
                                peer.deviceName,
                                peer.fingerprint,
                                peer.publicKey,
                                peer.dhPublicKey
                            )
                            accepted.set(true)
                        } catch (_: Throwable) {
                            accepted.set(false)
                        }
                    }
                    latch.countDown()
                }
            }
            val prompt = PendingIncomingPeer(requestId, peer, complete)
            mainHandler.post { pendingIncomingPeer = prompt }
            val completed = try {
                latch.await(120, TimeUnit.SECONDS)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                false
            }
            if (!completed) {
                resolved.compareAndSet(false, true)
            }
            mainHandler.post {
                if (pendingIncomingPeer?.requestId == requestId) {
                    pendingIncomingPeer = null
                }
            }
            completed && accepted.get()
        }
        onDispose {
            RustBridge.onIncomingPeerListener = null
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("LinkHub") },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.primaryContainer
                )
            )
        },
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    icon = { Text("📷") },
                    label = { Text("配对") },
                    selected = currentTab == Tab.Pair,
                    onClick = { currentTab = Tab.Pair }
                )
                NavigationBarItem(
                    icon = { Text("💻") },
                    label = { Text("设备") },
                    selected = currentTab == Tab.Devices,
                    onClick = { currentTab = Tab.Devices }
                )
                NavigationBarItem(
                    icon = { Text("📤") },
                    label = { Text("发送") },
                    selected = currentTab == Tab.Send,
                    onClick = { currentTab = Tab.Send }
                )
                NavigationBarItem(
                    icon = { Text("🧾") },
                    label = { Text("历史") },
                    selected = currentTab == Tab.History,
                    onClick = { currentTab = Tab.History }
                )
                NavigationBarItem(
                    icon = { Text("⚙") },
                    label = { Text("服务") },
                    selected = currentTab == Tab.Service,
                    onClick = { currentTab = Tab.Service }
                )
            }
        }
    ) { padding ->
        Box(modifier = Modifier.padding(padding)) {
            when (currentTab) {
                Tab.Pair -> PairScreen()
                Tab.Devices -> DevicesScreen()
                Tab.Send -> SendScreen(sharedUris, onSharedUrisConsumed)
                Tab.History -> HistoryScreen()
                Tab.Service -> ServiceScreen()
            }
        }
    }

    pendingIncomingPeer?.let { prompt ->
        AlertDialog(
            onDismissRequest = {
                prompt.complete(false)
                pendingIncomingPeer = null
            },
            title = { Text("接受附近设备?") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(prompt.peer.deviceName.ifBlank { prompt.peer.deviceId })
                    Text("安全码: ${prompt.peer.fingerprint}")
                    Text(
                        "设备 ID: ${prompt.peer.deviceId}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    prompt.complete(true)
                    pendingIncomingPeer = null
                }) {
                    Text("接受")
                }
            },
            dismissButton = {
                TextButton(onClick = {
                    prompt.complete(false)
                    pendingIncomingPeer = null
                }) {
                    Text("拒绝")
                }
            }
        )
    }
}

@Suppress("DEPRECATION")
private fun shareUrisFromIntent(intent: Intent?): List<Uri> {
    if (intent == null) return emptyList()
    return when (intent.action) {
        Intent.ACTION_SEND -> listOfNotNull(intent.getParcelableExtra(Intent.EXTRA_STREAM))
        Intent.ACTION_SEND_MULTIPLE -> {
            intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.toList().orEmpty()
        }
        else -> emptyList()
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkHubTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = lightColorScheme(),
        content = content
    )
}
