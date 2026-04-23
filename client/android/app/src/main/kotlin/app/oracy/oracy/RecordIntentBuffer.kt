package app.oracy.oracy

class RecordIntentBuffer {
    private var flutterReady = false
    private var hasPendingRecordIntent = false

    fun recordIntentReceived(dispatchImmediately: () -> Unit) {
        if (flutterReady) {
            dispatchImmediately()
            return
        }

        hasPendingRecordIntent = true
    }

    fun consumePendingRecordIntent(): Boolean {
        flutterReady = true
        val hadPendingIntent = hasPendingRecordIntent
        hasPendingRecordIntent = false
        return hadPendingIntent
    }
}
