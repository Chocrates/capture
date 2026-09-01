package dev.capture

import android.content.Context
import android.media.MediaRecorder
import android.os.Build
import java.io.File

/**
 * Recording, for as long as a finger is down.
 *
 * §2.2: the phone's job ends at "the audio exists". Nothing here transcribes,
 * and nothing here decides what the recording is about — a transcript is
 * enrichment that arrives later as an edit, and §4.3 forbids deriving the
 * note's permanent name from one, because machine output changes and a name
 * must not.
 *
 * The file is a scratch file. The archive takes the bytes and stores them
 * under their own hash, so this one is deleted immediately afterwards: two
 * copies of a recording, one of them unmanaged and outside eviction, is how a
 * phone fills up with things nothing knows about.
 */
class Recorder(private val context: Context) {

    private var recorder: MediaRecorder? = null
    private var file: File? = null
    private var startedAt: Long = 0

    val running: Boolean
        get() = recorder != null

    /**
     * Begin.
     *
     * OGG/Opus, which every Android since 29 can write and every platform can
     * read. Chosen over AMR — the historical default for voice — because AMR
     * is narrowband and this is somebody's own thinking, kept for years, not a
     * phone call.
     */
    fun start(): Boolean {
        if (running) return true

        val scratch = File(context.cacheDir, "recording-${System.nanoTime()}.ogg")
        val r = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            MediaRecorder(context)
        } else {
            @Suppress("DEPRECATION")
            MediaRecorder()
        }

        return try {
            r.setAudioSource(MediaRecorder.AudioSource.MIC)
            r.setOutputFormat(MediaRecorder.OutputFormat.OGG)
            r.setAudioEncoder(MediaRecorder.AudioEncoder.OPUS)
            r.setAudioSamplingRate(48_000)
            r.setAudioEncodingBitRate(32_000)
            r.setOutputFile(scratch.absolutePath)
            r.prepare()
            r.start()
            recorder = r
            file = scratch
            startedAt = System.currentTimeMillis()
            true
        } catch (e: Exception) {
            // Nothing was recorded, and saying so is the point: a hold-to-talk
            // that silently records nothing is worse than one that refuses,
            // because the person has already said the thing.
            r.release()
            scratch.delete()
            recorder = null
            file = null
            false
        }
    }

    /**
     * Stop, and hand back what was said.
     *
     * `null` when there is nothing worth keeping — a tap rather than a hold,
     * or a recorder that failed. An empty note with an empty recording is
     * still a note somebody has to deal with later.
     */
    fun stop(): Said? {
        val r = recorder ?: return null
        val scratch = file
        recorder = null
        file = null

        val duration = System.currentTimeMillis() - startedAt
        try {
            r.stop()
        } catch (e: RuntimeException) {
            // `stop` throws when the recording is too short to have produced a
            // valid file, and the file it leaves is unusable rather than
            // short.
            r.release()
            scratch?.delete()
            return null
        }
        r.release()

        val bytes = scratch?.takeIf { it.exists() }?.readBytes()
        // Deleted whether or not it is used. The archive stores the bytes
        // under their own hash; a second copy outside that is a copy nothing
        // manages and eviction cannot reach.
        scratch?.delete()

        if (bytes == null || bytes.isEmpty()) return null
        return Said(bytes, duration.toUInt())
    }

    /** Give up without keeping anything. */
    fun cancel() {
        val r = recorder ?: return
        recorder = null
        try {
            r.stop()
        } catch (e: RuntimeException) {
            // Already not recording. Nothing to do but release it.
        }
        r.release()
        file?.delete()
        file = null
    }

    /** What was recorded. */
    data class Said(val bytes: ByteArray, val durationMs: UInt) {
        // Overridden because the compiler will not do it for an array, and the
        // default identity comparison on a data class is a trap somebody will
        // fall into eventually.
        override fun equals(other: Any?): Boolean =
            this === other ||
                (other is Said && bytes.contentEquals(other.bytes) && durationMs == other.durationMs)

        override fun hashCode(): Int = 31 * bytes.contentHashCode() + durationMs.hashCode()
    }

    companion object {
        /** What the archive is told these bytes are. */
        const val MEDIA_TYPE = "audio/ogg"

        /**
         * Below this, a hold was a tap.
         *
         * Not a debounce on the button — the platform reports the press
         * faithfully. It is that a recorder given a few hundred milliseconds
         * produces a file with no audio in it, and a note whose recording is
         * silence is a note somebody has to open to find out it is nothing.
         */
        const val SHORTEST_MS = 500L
    }
}
