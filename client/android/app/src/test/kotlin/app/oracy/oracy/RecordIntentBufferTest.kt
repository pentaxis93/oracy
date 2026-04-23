package app.oracy.oracy

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RecordIntentBufferTest {
    @Test
    fun givenRecordIntentArrivesBeforeFlutterIsReady_whenFlutterConsumesPendingIntent_thenItIsReturnedOnce() {
        val buffer = RecordIntentBuffer()
        var immediateDispatches = 0

        buffer.recordIntentReceived { immediateDispatches++ }

        assertEquals(0, immediateDispatches)
        assertTrue(buffer.consumePendingRecordIntent())
        assertFalse(buffer.consumePendingRecordIntent())
    }

    @Test
    fun givenFlutterIsReady_whenRecordIntentArrives_thenItDispatchesImmediatelyWithoutPendingReplay() {
        val buffer = RecordIntentBuffer()
        var immediateDispatches = 0

        assertFalse(buffer.consumePendingRecordIntent())
        buffer.recordIntentReceived { immediateDispatches++ }

        assertEquals(1, immediateDispatches)
        assertFalse(buffer.consumePendingRecordIntent())
    }
}
