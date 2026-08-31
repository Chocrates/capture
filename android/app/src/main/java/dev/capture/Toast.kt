package dev.capture

import android.content.Context
import android.widget.Toast as AndroidToast

/** Say something briefly, without a screen to say it on. */
object Toast {
    fun short(context: Context, text: String) =
        AndroidToast.makeText(context, text, AndroidToast.LENGTH_SHORT).show()
}
