package dev.capture

import android.content.Context
import uniffi.capture_ffi.Archive

/**
 * The one archive this process talks to.
 *
 * Opened once and kept, because opening it does real work — it resolves the
 * store, loads the device identity, and creates everything on first run — and
 * the capture surfaces are the last place to spend that. The handle itself
 * holds no connection between calls, so keeping it costs nothing beyond the
 * paths it remembers.
 *
 * `filesDir` is application-private storage: not readable by other apps, gone
 * when the app is uninstalled, and never on external storage where anything
 * could read it. The archive is the whole of what someone has written down and
 * has no business anywhere else.
 */
object Store {
    @Volatile private var archive: Archive? = null

    fun of(context: Context): Archive =
        archive ?: synchronized(this) {
            archive ?: Archive.open(context.applicationContext.filesDir.absolutePath)
                .also { archive = it }
        }

    /**
     * Open it now, off the main thread, so the first capture does not pay for
     * it.
     *
     * Opening resolves the store, loads the device identity and creates
     * everything on first run. That is real work, and the capture surfaces are
     * the last place to spend it — §2.1's argument is that capture costs
     * almost nothing at the moment a thought arrives, and "almost nothing plus
     * opening a database" is not that.
     *
     * Safe to call repeatedly: the second call finds it open.
     */
    fun warm(context: Context) {
        val app = context.applicationContext
        Thread { runCatching { of(app) } }.start()
    }
}
