package app.oracy.oracy

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WidgetRecordIntentAuthenticatorTest {
    private val authenticator = WidgetRecordIntentAuthenticator(
        expectedAction = "app.oracy.oracy.ACTION_RECORD",
        expectedToken = "owned-widget-token"
    )

    @Test
    fun givenWidgetRecordIntentHasExpectedToken_whenAuthenticated_thenItIsAccepted() {
        assertTrue(
            authenticator.isAuthenticatedRecordIntent(
                action = "app.oracy.oracy.ACTION_RECORD",
                token = "owned-widget-token"
            )
        )
    }

    @Test
    fun givenWidgetRecordIntentHasNoToken_whenAuthenticated_thenItIsRejected() {
        assertFalse(
            authenticator.isAuthenticatedRecordIntent(
                action = "app.oracy.oracy.ACTION_RECORD",
                token = null
            )
        )
    }

    @Test
    fun givenWidgetRecordIntentHasWrongToken_whenAuthenticated_thenItIsRejected() {
        assertFalse(
            authenticator.isAuthenticatedRecordIntent(
                action = "app.oracy.oracy.ACTION_RECORD",
                token = "spoofed-token"
            )
        )
    }

    @Test
    fun givenIntentHasDifferentAction_whenAuthenticated_thenItIsRejected() {
        assertFalse(
            authenticator.isAuthenticatedRecordIntent(
                action = "android.intent.action.MAIN",
                token = "owned-widget-token"
            )
        )
    }
}
