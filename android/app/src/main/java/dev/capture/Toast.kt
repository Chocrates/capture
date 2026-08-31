package dev.capture

import android.content.Context
import android.widget.Toast as AndroidToast

/** Say something briefly, without a screen to say it on. */
object Toast {
    fun short(context: Context, text: String) =
        AndroidToast.makeText(context, text, AndroidToast.LENGTH_SHORT).show()

    /**
     * Say something from a background worker, which has no screen at all.
     *
     * On the main looper because a worker does not run there, and a toast
     * raised off it is dropped silently — which would make the one message
     * that exists to prevent a silent failure fail silently.
     */
    fun note(context: Context, text: String) =
        android.os.Handler(android.os.Looper.getMainLooper()).post {
            AndroidToast.makeText(context, text, AndroidToast.LENGTH_LONG).show()
        }
}
