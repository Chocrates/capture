package dev.capture

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.capture_ffi.Archive
import uniffi.capture_ffi.Captured

/**
 * The phone and a desktop reaching the same archive.
 *
 * M3 proved convergence between two directories on one machine. That is a real
 * test of the rules and says nothing about the thing most likely to be wrong
 * here: two different platforms, two different builds of SQLite, two different
 * tz databases, two different notions of what a temporary directory is, on
 * opposite ends of a socket.
 *
 * The desktop half is a real `capture` process driven by the harness in
 * `tools/converge.sh`, which also arranges the ports. Without that harness
 * these tests skip rather than fail: a device with no desktop to talk to has
 * not found a bug, and a red suite that means "you didn't run the other half"
 * is a suite people learn to ignore.
 */
@RunWith(AndroidJUnit4::class)
class ConvergeTest {

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    /**
     * Where the desktop is, and the code it is showing.
     *
     * Passed as instrumentation arguments because the harness knows them and
     * this process cannot discover them: the emulator's network is not the
     * host's, and the two are joined by `adb reverse` rather than by anything
     * either side can see.
     */
    private fun arg(name: String): String? =
        InstrumentationRegistry.getArguments().getString(name)

    @Test
    fun the_phone_pairs_with_a_desktop_and_both_hold_both_notes() {
        val code = arg("pairingCode")
        val addr = arg("pairingAddr")
        assumeTrue("no desktop half; run tools/converge.sh", code != null && addr != null)

        val dir = java.io.File(context.filesDir, "converge-${System.nanoTime()}")
        dir.mkdirs()
        val phone = Archive.open(dir.absolutePath)

        val mine = when (val c = phone.capture("written on the phone")) {
            is Captured.Stored -> c.slug
            else -> throw AssertionError("spooled rather than stored")
        }

        phone.joinWithCode(addr!!, code!!)
        assertEquals("the desktop is now a peer", 1, phone.peers().size)

        // Several rounds. One is enough when both sides are awake and the
        // network cooperates, and the point of this test is the case where
        // that is not a safe assumption.
        repeat(3) { phone.sync(10u) }

        // What the desktop wrote, before this device existed.
        val theirs = arg("desktopSlug")!!
        assertNotNull(
            "the phone never received the note the desktop captured",
            phone.note(theirs).text,
        )

        // And the harness asserts the other direction, because only it can see
        // the desktop's archive.
        println("CONVERGE_PHONE_SLUG=$mine")
        println("CONVERGE_PHONE_COUNT=${phone.noteCount()}")
    }
}
