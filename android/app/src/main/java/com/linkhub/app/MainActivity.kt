package com.linkhub.app

import android.Manifest
import android.os.Bundle
import android.os.Build
import android.content.pm.PackageManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import com.linkhub.app.ui.DevicesScreen
import com.linkhub.app.ui.HistoryScreen
import com.linkhub.app.ui.PairScreen
import com.linkhub.app.ui.SendScreen
import com.linkhub.app.ui.ServiceScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestNotificationPermissionIfNeeded()
        setContent {
            LinkHubTheme {
                LinkHubMain()
            }
        }
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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkHubMain() {
    var currentTab by remember { mutableStateOf(Tab.Pair) }

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
                Tab.Send -> SendScreen()
                Tab.History -> HistoryScreen()
                Tab.Service -> ServiceScreen()
            }
        }
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
