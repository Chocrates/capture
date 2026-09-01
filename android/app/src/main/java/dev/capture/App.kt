package dev.capture

import android.app.Application

/**
 * The process, and the one thing worth doing before anybody asks.
 *
 * Opening the archive resolves the store, loads the device identity and
 * creates everything on first run. Done lazily, that work lands on whichever
 * surface is used first — and the surface used first is usually a capture,
 * which is the one place §2.1 says must cost almost nothing.
 *
 * So it happens here, off the main thread, while the window is still being
 * laid out. Nothing waits for it: a capture that arrives before it finishes
 * blocks on the same lock and is no worse off than it would have been.
 */
class App : Application() {
    override fun onCreate() {
        super.onCreate()
        Store.warm(this)
    }
}
