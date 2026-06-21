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
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
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
                title = {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp)
                    ) {
                        // CSS-free brand mark: a small indigo→violet gradient tile,
                        // matching the desktop shell so the two clients read as one.
                        Box(
                            modifier = Modifier
                                .size(28.dp)
                                .clip(RoundedCornerShape(9.dp))
                                .background(
                                    Brush.linearGradient(listOf(BrandIndigoLight, BrandViolet))
                                )
                        )
                        Text("LinkHub", fontWeight = FontWeight.Bold)
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                    titleContentColor = MaterialTheme.colorScheme.onSurface
                )
            )
        },
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.QrCodeScanner, contentDescription = null) },
                    label = { Text("配对") },
                    selected = currentTab == Tab.Pair,
                    onClick = { currentTab = Tab.Pair }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Devices, contentDescription = null) },
                    label = { Text("设备") },
                    selected = currentTab == Tab.Devices,
                    onClick = { currentTab = Tab.Devices }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Send, contentDescription = null) },
                    label = { Text("发送") },
                    selected = currentTab == Tab.Send,
                    onClick = { currentTab = Tab.Send }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.History, contentDescription = null) },
                    label = { Text("历史") },
                    selected = currentTab == Tab.History,
                    onClick = { currentTab = Tab.History }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Tune, contentDescription = null) },
                    label = { Text("服务") },
                    selected = currentTab == Tab.Service,
                    onClick = { currentTab = Tab.Service }
                )
            }
        }
    ) { padding ->
        AnimatedContent(
            targetState = currentTab,
            transitionSpec = { fadeIn(tween(220)) togetherWith fadeOut(tween(160)) },
            label = "tab-content",
            modifier = Modifier.padding(padding),
        ) { tab ->
            when (tab) {
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

// Brand palette — kept in sync with the desktop shell (indigo → violet) so the
// two clients read as one product.
private val BrandIndigo = Color(0xFF4361EE)
private val BrandIndigoLight = Color(0xFF5B7CFA)
private val BrandViolet = Color(0xFF8B5CF6)
private val BrandCyan = Color(0xFF06B6D4)

private val LinkHubLightColors = lightColorScheme(
    primary = BrandIndigo,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFE0E6FF),
    onPrimaryContainer = Color(0xFF101A4D),
    secondary = BrandViolet,
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFEDE4FF),
    onSecondaryContainer = Color(0xFF2A124D),
    tertiary = BrandCyan,
    background = Color(0xFFF6F7FB),
    onBackground = Color(0xFF161826),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF161826),
    surfaceVariant = Color(0xFFECEEF5),
    onSurfaceVariant = Color(0xFF5C6275),
    outline = Color(0xFFC7CCDA),
)

private val LinkHubDarkColors = darkColorScheme(
    primary = BrandIndigoLight,
    onPrimary = Color(0xFF0A1033),
    primaryContainer = Color(0xFF2A3578),
    onPrimaryContainer = Color(0xFFDDE3FF),
    secondary = Color(0xFFB79CFF),
    onSecondary = Color(0xFF1F1140),
    secondaryContainer = Color(0xFF3A2B66),
    onSecondaryContainer = Color(0xFFEADDFF),
    tertiary = Color(0xFF5FD4E6),
    background = Color(0xFF0C0E16),
    onBackground = Color(0xFFEEF0F7),
    surface = Color(0xFF151823),
    onSurface = Color(0xFFEEF0F7),
    surfaceVariant = Color(0xFF272C3B),
    onSurfaceVariant = Color(0xFFA7ADC0),
    outline = Color(0xFF3A4152),
)

@Composable
fun LinkHubTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) LinkHubDarkColors else LinkHubLightColors,
        content = content,
    )
}
