package dev.capture

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.capture_ffi.Captured

/**
 * Capture something shared out of another application.
 *
 * No screen at all. A share sheet is already a deliberate act with the content
 * chosen — putting a confirmation behind it would ask the same question twice,
 * and §2.1's argument is that every extra decision costs more than the thought
 * is worth. It captures and gets out of the way.
 *
 * The URL goes into the ambient envelope rather than into the text (§2.3). A
 * quotation without its source is something you cannot go back to; a URL
 * pasted into the text becomes part of what you wrote, and would then be
 * searched, edited and redacted as though you had typed it.
 */
class ShareActivity : ComponentActivity() {

    /** When this arrived, so what it costs can be measured rather than claimed. */
    private var began: Long = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        began = System.currentTimeMillis()

        val text = intent?.shared()
        if (text.isNullOrBlank()) {
            // Nothing to capture. Said rather than passing silently: a share
            // that appears to succeed and stores nothing is worse than one
            // that admits it did not understand what it was given.
            Toast.short(this, "Nothing to capture in that.")
            finish()
            return
        }

        lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) {
                Store.of(this@ShareActivity).captureShared(text, intent?.sourceUrl())
            }
            Toast.short(
                this@ShareActivity,
                when (result) {
                    is Captured.Stored -> "Captured."
                    is Captured.Spooled -> "Saved. It will be filed when the store is writable."
                },
            )
            // What the share gesture cost, gesture to committed. Reported
            // separately from the tile because it starts with the content
            // already chosen and so should be the cheapest surface there is —
            // a claim worth checking rather than assuming.
            runCatching {
                Store.of(this@ShareActivity)
                    .took("share", (System.currentTimeMillis() - began).toULong())
            }
            finish()
        }
    }
}

/** The text an application shared, whatever field it used to send it. */
private fun Intent.shared(): String? = when (action) {
    Intent.ACTION_SEND -> getStringExtra(Intent.EXTRA_TEXT)
    Intent.ACTION_PROCESS_TEXT -> getStringExtra(Intent.EXTRA_PROCESS_TEXT)
    else -> null
}

/**
 * Where it came from, if the sharing application said.
 *
 * Browsers put the page URL in `EXTRA_TEXT` and the title in `EXTRA_SUBJECT`,
 * which means the URL is usually *also* the text. That is not a reason to strip
 * it: the text is what was shared and should be stored as shared. Recording it
 * in the envelope as well is what makes it a source rather than a coincidence.
 */
private fun Intent.sourceUrl(): String? {
    val text = getStringExtra(Intent.EXTRA_TEXT)?.trim() ?: return null
    // The last whitespace-separated token that looks like a link. Sharing
    // "look at this https://example.com/x" puts the URL at the end; sharing
    // from a browser makes the whole text the URL.
    return text.split(Regex("\\s+")).lastOrNull { it.startsWith("http://") || it.startsWith("https://") }
}
