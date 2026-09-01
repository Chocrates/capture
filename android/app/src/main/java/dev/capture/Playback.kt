package dev.capture

import android.media.MediaPlayer
import android.content.Context
import java.io.File

/**
 * Hearing a recording back.
 *
 * The bytes live in the archive under their own hash, and `MediaPlayer` wants
 * a file, so this writes a scratch copy and deletes it when playback ends. Not
 * a cache: a second managed copy is a copy eviction cannot reach, and the
 * whole point of §5.6's eviction is that a constrained device can drop bytes
 * and get them back.
 */
object Playback {
    private var player: MediaPlayer? = null
    private var scratch: File? = null

    /** Play these bytes, stopping whatever was playing. */
    fun play(context: Context, bytes: ByteArray, whenDone: () -> Unit) {
        stop()

        val f = File(context.cacheDir, "playing-${System.nanoTime()}.ogg")
        f.writeBytes(bytes)
        scratch = f

        player = MediaPlayer().apply {
            setDataSource(f.absolutePath)
            setOnCompletionListener {
                whenDone()
                stop()
            }
            setOnErrorListener { _, _, _ ->
                whenDone()
                stop()
                true
            }
            prepare()
            start()
        }
    }

    fun stop() {
        player?.let {
            runCatching { it.stop() }
            it.release()
        }
        player = null
        scratch?.delete()
        scratch = null
    }
}
