package app.oracy.oracy

import android.content.Context
import java.util.UUID

internal object WidgetRecordIntentContract {
    const val ACTION_RECORD = "app.oracy.oracy.ACTION_RECORD"
    const val EXTRA_RECORD_TOKEN = "app.oracy.oracy.extra.RECORD_TOKEN"
}

internal object WidgetRecordIntentTokens {
    private const val PREFERENCES_NAME = "oracy_widget_record_intents"
    private const val RECORD_TOKEN_KEY = "record_token"

    fun getOrCreate(context: Context): String {
        val preferences = context.applicationContext.getSharedPreferences(
            PREFERENCES_NAME,
            Context.MODE_PRIVATE
        )
        val existingToken = preferences.getString(RECORD_TOKEN_KEY, null)
        if (existingToken != null) {
            return existingToken
        }

        val token = UUID.randomUUID().toString()
        preferences.edit().putString(RECORD_TOKEN_KEY, token).apply()
        return token
    }
}

internal class WidgetRecordIntentAuthenticator(
    private val expectedAction: String,
    private val expectedToken: String
) {
    fun isAuthenticatedRecordIntent(action: String?, token: String?): Boolean {
        return action == expectedAction && token == expectedToken
    }
}
