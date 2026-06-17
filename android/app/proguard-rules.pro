# LinkHub ProGuard / R8 rules for release (minify + resource shrink) builds.

# --- JNI bridge ---------------------------------------------------------
# RustBridge holds external (native) methods resolved by name from the Rust
# .so, plus onFileReceived() which the native listener thread calls back into.
# R8 must not rename, remove, or repackage any of these or JNI linkage breaks.
-keep class com.linkhub.app.bridge.RustBridge { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}

# --- Jetpack Compose ----------------------------------------------------
# Compose ships its own consumer rules via the AndroidX artifacts, but keep
# @Composable metadata defensively for tooling/reflection.
-keep,allowshrinking class androidx.compose.** { *; }
-dontwarn androidx.compose.**

# --- Gson (used for identity / trust store / history serialization) -----
# Keep generic signatures and annotations so reflective (de)serialization of
# model classes survives shrinking.
-keepattributes Signature, *Annotation*, EnclosingMethod, InnerClasses

# Gson DTOs are populated purely by reflection, so R8's static analysis sees
# their fields/constructors as unused and strips/renames them — which makes
# gson.fromJson return null/throw at runtime (e.g. "创建失败" on identity gen).
# Keep these model classes and their members intact. Covers fields both with
# and without @SerializedName.
-keep class com.linkhub.app.ui.IdentityJson { *; }
-keep class com.linkhub.app.ui.PeerInfoJson { *; }
-keep class com.linkhub.app.ui.PairResultJson { *; }
-keep class com.linkhub.app.ui.TrustedPeer { *; }
-keep class com.linkhub.app.ui.SendResultJson { *; }
-keep class com.linkhub.app.ui.TransmissionHistoryEntry { *; }
-keep class com.linkhub.app.ui.DiscoveredPeerAddress { *; }
-keep class com.linkhub.app.ui.AndroidNetworkHint { *; }
-keep class com.linkhub.app.service.LinkHubService$ListenerResult { *; }

# Belt-and-suspenders: never strip/rename any field annotated for Gson.
-keepclassmembers class * {
    @com.google.gson.annotations.SerializedName <fields>;
}

# --- General ------------------------------------------------------------
# Preserve line numbers for readable release stack traces.
-keepattributes SourceFile, LineNumberTable
-renamesourcefileattribute SourceFile
