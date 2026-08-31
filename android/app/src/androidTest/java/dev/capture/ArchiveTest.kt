package dev.capture

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.capture_ffi.Archive
import uniffi.capture_ffi.Captured

/**
 * The binding, called the way the application will call it.
 *
 * The Rust side has its own tests and they pass. This asks a different
 * question: does the generated Kotlin, loading a real `.so` through JNA, on a
 * real Android runtime, do what the Rust did? Every layer between here and
 * there — the generator, JNA, the ABI, the packaging — is untested by the
 * Rust suite and is exactly where a binding goes wrong.
 */
@RunWith(AndroidJUnit4::class)
class ArchiveTest {

    /** A directory the application owns, as the real thing will use. */
    private fun scratch(name: String): String {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val dir = java.io.File(context.filesDir, "test-$name-${System.nanoTime()}")
        dir.deleteRecursively()
        return dir.absolutePath
    }

    @Test
    fun opensAnArchiveAndCapturesThroughTheBinding() {
        val archive = Archive.open(scratch("capture"))

        val result = archive.capture("captured through the generated Kotlin")
        assertTrue("expected a stored capture, got $result", result is Captured.Stored)

        val slug = (result as Captured.Stored).slug
        val note = archive.note(slug)
        assertEquals("captured through the generated Kotlin", note.text)
        assertEquals(1uL, archive.noteCount())
    }

    @Test
    fun aCaptureSurvivesTheHandleGoingAway() {
        // The shape Android actually produces: a Quick Settings tile is
        // destroyed the moment its callback returns. Anything queued on this
        // side rather than committed is lost exactly here.
        val root = scratch("durable")
        Archive.open(root).use { it.capture("written by something about to die") }

        Archive.open(root).use { assertEquals(1uL, it.noteCount()) }
    }

    @Test
    fun theTimelineComesBackNewestFirst() {
        val archive = Archive.open(scratch("timeline"))
        archive.capture("first")
        archive.capture("second")

        val rows = archive.timeline(10u)
        assertEquals(2, rows.size)
        assertEquals("second", rows[0].text)
    }

    @Test
    fun aQueuedDestructionIsNotAFinishedOne() {
        // The distinction one flag would lose, checked across the boundary:
        // between ordering and executing, the content is withheld and still
        // recoverable, and that is the state a person can still act on.
        val archive = Archive.open(scratch("redact"))
        val slug = (archive.capture("something regrettable") as Captured.Stored).slug

        val cancelId = archive.redact(slug)
        val queued = archive.note(slug)
        assertTrue("ordered", queued.redactionQueued)
        assertTrue("not yet carried out", !queued.redacted)
        assertNull("withheld in the meantime", queued.text)

        archive.cancelRedaction(cancelId)
        assertTrue("the window is real", !archive.note(slug).redactionQueued)
    }

    @Test
    fun everyMutationIsReachable() {
        // §8.4 and §2.6: muting must not cost more than destroying, and it
        // costs more the moment it needs a different machine.
        val archive = Archive.open(scratch("mutations"))
        val slug = (archive.capture("a note to work on") as Captured.Stored).slug

        archive.edit(slug, "edited on the phone")
        assertEquals("edited on the phone", archive.note(slug).text)

        archive.tag(slug, "errand")
        archive.untag(slug, "errand")
        archive.setAlias(slug, "the-thing")
        assertEquals(slug, archive.note("the-thing").slug)

        archive.pin(slug, true)
        assertTrue(archive.note(slug).pinned)

        archive.mute(slug)
        assertNotNull(archive.note(slug).mutedUntilMs)
    }

    @Test
    fun theDeviceKeepsItsIdentityAcrossOpens() {
        // A phone is a device, not a session. A fresh identity per open would
        // give one machine many origins, and every peer would have to learn
        // each of them.
        val root = scratch("identity")
        val first = Archive.open(root).use { it.deviceKey() }
        val second = Archive.open(root).use { it.deviceKey() }
        assertEquals(first, second)
    }
}
