# What R8 must not remove.
#
# uniffi's generated Kotlin reaches the native library through JNA, which finds
# its methods by name at runtime. R8 cannot see those uses — there is no call
# site in the bytecode — so without these it removes classes the application
# depends on and the failure appears on the first call rather than at build
# time.

# The generated binding, and the callback interfaces JNA maps onto it.
-keep class uniffi.capture_ffi.** { *; }

# JNA itself, for the same reason: it is reflection all the way down.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }

# Names that appear in a stack trace are worth keeping. A crash report from a
# device that says only `a.b.c(:1)` is a crash report nobody can act on, and
# this file exists to make the application smaller rather than harder to debug.
-keepattributes SourceFile,LineNumberTable

# JNA is compiled against a desktop JVM and refers to AWT, which does not exist
# on Android and is never reached there — the code paths that mention it are
# for platforms this application does not run on. R8 is right that the classes
# are missing and wrong that it matters.
-dontwarn java.awt.**
-dontwarn javax.swing.**
