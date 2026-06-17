package com.linkhub.app.ui

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.google.gson.Gson
import com.google.gson.annotations.SerializedName
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import com.linkhub.app.bridge.RustBridge
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

data class IdentityJson(
    @SerializedName("device_id") val deviceId: String = "",
    @SerializedName("device_name") val deviceName: String = "",
    @SerializedName("fingerprint") val fingerprint: String = "",
    @SerializedName("public_key") val publicKey: String = "",
    @SerializedName("dh_public_key") val dhPublicKey: String = "",
    @SerializedName("signing_key_hex") val signingKeyHex: String = "",
    @SerializedName("static_dh_key_hex") val staticDhKeyHex: String = "",
    @SerializedName("created_at_secs") val createdAtSecs: Long = 0
)

data class PeerInfoJson(
    @SerializedName("device_id") val deviceId: String = "",
    @SerializedName("device_name") val deviceName: String = "",
    @SerializedName("fingerprint") val fingerprint: String = "",
    @SerializedName("confirmation_code") val confirmationCode: String = ""
)

data class PairResultJson(
    val success: Boolean = false,
    @SerializedName("device_id") val deviceId: String = "",
    @SerializedName("device_name") val deviceName: String = "",
    val fingerprint: String = "",
    val error: String = ""
)

fun copyToClipboard(context: Context, label: String, text: String) {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    cm.setPrimaryClip(ClipData.newPlainText(label, text))
}

@Composable
fun CopyButton(text: String, label: String) {
    val ctx = LocalContext.current
    IconButton(onClick = { copyToClipboard(ctx, label, text) }, modifier = Modifier.size(32.dp)) {
        Icon(Icons.Default.ContentCopy, contentDescription = "复制", modifier = Modifier.size(16.dp))
    }
}

@Composable
fun PairScreen() {
    val ctx = LocalContext.current
    val gson = remember { Gson() }
    var deviceName by remember { mutableStateOf("我的安卓") }

    // Load saved identity on startup.
    var identity by remember { mutableStateOf<IdentityJson?>(null) }
    var identityLoaded by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        if (!identityLoaded) {
            try {
                identity = loadIdentity(ctx)
            } catch (_: Exception) {}
            identityLoaded = true
        }
    }
    var myPayload by remember { mutableStateOf("") }
    var peerPayload by remember { mutableStateOf("") }
    var peerInfo by remember { mutableStateOf<PeerInfoJson?>(null) }
    var confirmationInput by remember { mutableStateOf("") }
    var statusMsg by remember { mutableStateOf("") }
    var pairResult by remember { mutableStateOf<PairResultJson?>(null) }
    var scannerOpen by remember { mutableStateOf(false) }
    var cameraGranted by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(ctx, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    val cameraPermissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        cameraGranted = granted
        scannerOpen = granted
        statusMsg = if (granted) "相机已授权，请扫描对方二维码" else "未授予相机权限，可继续手动粘贴配对码"
    }

    fun inspectPeerPayload(payload: String) {
        if (identity == null || payload.isBlank()) return
        val json = RustBridge.parsePairingPayload(gson.toJson(identity), payload)
        peerInfo = try { gson.fromJson(json, PeerInfoJson::class.java) } catch (_: Exception) { null }
        confirmationInput = ""
        pairResult = null
        statusMsg = if (peerInfo != null) "确认码: ${peerInfo!!.confirmationCode}" else "解析失败"
    }

    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Text("本机身份", style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(value = deviceName, onValueChange = { deviceName = it },
            label = { Text("设备名称") }, modifier = Modifier.fillMaxWidth())
        Button(onClick = {
            val json = RustBridge.generateIdentity(deviceName)
            val id = try { gson.fromJson(json, IdentityJson::class.java) } catch (_: Exception) { null }
            if (id != null) {
                identity = id
                saveIdentity(ctx, json)
                statusMsg = "身份已保存: ${id.deviceId}"
            } else {
                statusMsg = "创建失败"
            }
        }) { Text("生成身份") }

        if (identity != null) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(modifier = Modifier.weight(1f)) {
                    Text("ID: ${identity!!.deviceId}", style = MaterialTheme.typography.bodySmall)
                    Text("指纹: ${identity!!.fingerprint}", style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                }
                CopyButton(identity!!.deviceId, "设备ID")
            }
        }

        Divider()

        Text("生成配对码", style = MaterialTheme.typography.titleMedium)
        Button(onClick = {
            if (identity != null) {
                myPayload = RustBridge.generatePairingPayload(gson.toJson(identity), 120)
                statusMsg = "配对码已生成 (有效期120秒)"
            }
        }, enabled = identity != null) { Text("生成配对码") }

        if (myPayload.isNotEmpty()) {
            PairingQrCode(myPayload)
            OutlinedTextField(value = myPayload, onValueChange = {},
                label = { Text("配对码 (发给对方)") }, modifier = Modifier.fillMaxWidth(),
                readOnly = true, maxLines = 3)
            Row(horizontalArrangement = Arrangement.End, modifier = Modifier.fillMaxWidth()) {
                TextButton(onClick = { copyToClipboard(ctx, "配对码", myPayload) }) { Text("📋 复制配对码") }
            }
        }

        Divider()

        Text("扫描对方", style = MaterialTheme.typography.titleMedium)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
            Button(
                onClick = {
                    if (cameraGranted) {
                        scannerOpen = true
                        statusMsg = "请扫描对方的 LinkHub 配对二维码"
                    } else {
                        cameraPermissionLauncher.launch(Manifest.permission.CAMERA)
                    }
                },
                enabled = identity != null
            ) {
                Text("扫码导入配对码")
            }
            if (scannerOpen) {
                TextButton(onClick = { scannerOpen = false }) {
                    Text("关闭扫码")
                }
            }
        }
        if (scannerOpen) {
            PairingPayloadScanner(
                onPayload = { payload ->
                    peerPayload = payload
                    scannerOpen = false
                    inspectPeerPayload(payload)
                },
                onInvalid = { statusMsg = "不是 LinkHub 配对码，请继续扫描" },
                onError = { statusMsg = "扫码失败: $it" }
            )
        }
        OutlinedTextField(value = peerPayload, onValueChange = { peerPayload = it },
            label = { Text("粘贴对方的配对码") }, modifier = Modifier.fillMaxWidth(), maxLines = 2)
        Button(onClick = {
            inspectPeerPayload(peerPayload)
        }, enabled = identity != null && peerPayload.isNotEmpty()) { Text("查看对方信息") }

        if (peerInfo != null) {
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(12.dp)) {
                    Text("设备: ${peerInfo!!.deviceName}", style = MaterialTheme.typography.titleSmall)
                    Text("ID: ${peerInfo!!.deviceId}")
                    Text("指纹: ${peerInfo!!.fingerprint}", fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("确认码: ${peerInfo!!.confirmationCode}",
                            style = MaterialTheme.typography.headlineMedium,
                            fontFamily = FontFamily.Monospace,
                            modifier = Modifier.weight(1f))
                        CopyButton(peerInfo!!.confirmationCode, "确认码")
                    }
                }
            }
            OutlinedTextField(value = confirmationInput, onValueChange = { confirmationInput = it },
                label = { Text("输入确认码") }, modifier = Modifier.fillMaxWidth())
            Button(onClick = {
                if (identity != null && peerPayload.isNotEmpty() && confirmationInput.isNotEmpty()) {
                    val json = RustBridge.confirmPairing(gson.toJson(identity), peerPayload, confirmationInput)
                    pairResult = try { gson.fromJson(json, PairResultJson::class.java) } catch (_: Exception) { null }
                    if (pairResult?.success == true) {
                        // Save peer to trusted list for Send page auto-fill
                        saveTrustedPeer(
                            ctx,
                            peerInfo!!.deviceId,
                            peerInfo!!.deviceName,
                            peerInfo!!.fingerprint,
                            peerPayload
                        )
                        statusMsg = "已信任: ${pairResult!!.deviceName}!"
                    } else {
                        statusMsg = "失败: ${pairResult?.error}"
                    }
                }
            }, enabled = confirmationInput.isNotEmpty()) { Text("确认配对") }
        }

        Divider()

        if (statusMsg.isNotEmpty()) {
            Text(statusMsg, color = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
fun PairingPayloadScanner(
    onPayload: (String) -> Unit,
    onInvalid: () -> Unit,
    onError: (String) -> Unit
) {
    val lifecycleOwner = LocalLifecycleOwner.current
    val executor = remember { Executors.newSingleThreadExecutor() }
    val handled = remember { AtomicBoolean(false) }

    DisposableEffect(Unit) {
        onDispose {
            handled.set(true)
            executor.shutdown()
        }
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.fillMaxWidth().padding(12.dp)) {
            Text("扫描 LinkHub 配对二维码", style = MaterialTheme.typography.titleSmall)
            Spacer(modifier = Modifier.height(8.dp))
            AndroidView(
                modifier = Modifier.fillMaxWidth().height(280.dp),
                factory = { context ->
                    val previewView = PreviewView(context).apply {
                        scaleType = PreviewView.ScaleType.FILL_CENTER
                    }
                    val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
                    cameraProviderFuture.addListener({
                        try {
                            val cameraProvider = cameraProviderFuture.get()
                            val preview = Preview.Builder().build().also {
                                it.setSurfaceProvider(previewView.surfaceProvider)
                            }
                            val analysis = ImageAnalysis.Builder()
                                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                                .build()
                                .also {
                                    it.setAnalyzer(executor) { imageProxy ->
                                        analyzePairingQr(imageProxy, handled, { payload ->
                                            if (handled.compareAndSet(false, true)) {
                                                onPayload(payload)
                                            }
                                        }, {
                                            if (!handled.get()) onInvalid()
                                        }, { error ->
                                            if (!handled.get()) onError(error)
                                        })
                                    }
                                }
                            cameraProvider.unbindAll()
                            cameraProvider.bindToLifecycle(
                                lifecycleOwner,
                                CameraSelector.DEFAULT_BACK_CAMERA,
                                preview,
                                analysis
                            )
                        } catch (e: Exception) {
                            onError(e.message ?: e::class.java.simpleName)
                        }
                    }, ContextCompat.getMainExecutor(context))
                    previewView
                }
            )
            Text(
                "将对方配对二维码放入框内；识别成功后会自动填入并显示确认码。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

private fun analyzePairingQr(
    imageProxy: ImageProxy,
    handled: AtomicBoolean,
    onPayload: (String) -> Unit,
    onInvalid: () -> Unit,
    onError: (String) -> Unit
) {
    if (handled.get()) {
        imageProxy.close()
        return
    }
    val mediaImage = imageProxy.image
    if (mediaImage == null) {
        imageProxy.close()
        return
    }
    val image = InputImage.fromMediaImage(mediaImage, imageProxy.imageInfo.rotationDegrees)
    BarcodeScanning.getClient()
        .process(image)
        .addOnSuccessListener { barcodes ->
            val payload = barcodes
                .asSequence()
                .filter { it.format == Barcode.FORMAT_QR_CODE || it.rawValue != null }
                .mapNotNull { it.rawValue?.trim() }
                .firstOrNull { it.startsWith("linkhub-pair-v2|") }
            if (payload != null) {
                onPayload(payload)
            } else if (barcodes.any { !it.rawValue.isNullOrBlank() }) {
                onInvalid()
            }
        }
        .addOnFailureListener { e ->
            onError(e.message ?: e::class.java.simpleName)
        }
        .addOnCompleteListener {
            imageProxy.close()
        }
}

@Composable
fun PairingQrCode(payload: String) {
    val qrBitmap = remember(payload) { pairingQrBitmap(payload, 720) }
    if (qrBitmap != null) {
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.fillMaxWidth().padding(12.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text("二维码配对", style = MaterialTheme.typography.titleSmall)
                Spacer(modifier = Modifier.height(8.dp))
                androidx.compose.foundation.Image(
                    bitmap = qrBitmap.asImageBitmap(),
                    contentDescription = "配对二维码",
                    modifier = Modifier.size(220.dp)
                )
            }
        }
    } else {
        Text("二维码生成失败，请复制配对码", color = MaterialTheme.colorScheme.error)
    }
}

private fun pairingQrBitmap(payload: String, size: Int): Bitmap? {
    return try {
        val matrix = QRCodeWriter().encode(payload, BarcodeFormat.QR_CODE, size, size)
        Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888).apply {
            for (x in 0 until size) {
                for (y in 0 until size) {
                    setPixel(x, y, if (matrix[x, y]) android.graphics.Color.BLACK else android.graphics.Color.WHITE)
                }
            }
        }
    } catch (_: Exception) {
        null
    }
}
