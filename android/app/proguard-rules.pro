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

# --- Gson (used for trust store / history serialization) ----------------
# Keep generic signatures and annotations so reflective (de)serialization of
# model classes survives shrinking.
-keepattributes Signature, *Annotation*, EnclosingMethod, InnerClasses

# --- General ------------------------------------------------------------
# Preserve line numbers for readable release stack traces.
-keepattributes SourceFile, LineNumberTable
-renamesourcefileattribute SourceFile
