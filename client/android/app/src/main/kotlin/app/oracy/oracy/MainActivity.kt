package app.oracy.oracy

import android.content.Intent
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    companion object {
        private const val CHANNEL = "app.oracy.oracy/widget"
    }

    private var methodChannel: MethodChannel? = null
    private val recordIntentBuffer = RecordIntentBuffer()

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        methodChannel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
        methodChannel?.setMethodCallHandler { call, result ->
            when (call.method) {
                "consumePendingRecordIntent" -> {
                    result.success(recordIntentBuffer.consumePendingRecordIntent())
                }
                else -> result.notImplemented()
            }
        }

        // Check if launched from widget
        handleIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        val authenticator = WidgetRecordIntentAuthenticator(
            expectedAction = WidgetRecordIntentContract.ACTION_RECORD,
            expectedToken = WidgetRecordIntentTokens.getOrCreate(this)
        )
        if (authenticator.isAuthenticatedRecordIntent(
                action = intent?.action,
                token = intent?.getStringExtra(WidgetRecordIntentContract.EXTRA_RECORD_TOKEN)
            )
        ) {
            recordIntentBuffer.recordIntentReceived {
                methodChannel?.invokeMethod("startRecordingFromWidget", null)
            }
        }
    }
}
