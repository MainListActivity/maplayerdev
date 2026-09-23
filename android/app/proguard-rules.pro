# iroh uniffi JNI classes must survive shrinking if minify is enabled later.
-keep class computer.iroh.** { *; }
-keepclasseswithmembernames class * { native <methods>; }
