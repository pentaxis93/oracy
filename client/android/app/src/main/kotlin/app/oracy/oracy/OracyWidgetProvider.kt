package app.oracy.oracy

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import es.antonborri.home_widget.HomeWidgetPlugin

/**
 * Widget provider for the Oracy home screen widget.
 *
 * Displays a quick-access record button that launches the app in recording mode.
 */
class OracyWidgetProvider : AppWidgetProvider() {

    override fun onUpdate(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetIds: IntArray
    ) {
        for (appWidgetId in appWidgetIds) {
            updateAppWidget(context, appWidgetManager, appWidgetId)
        }
    }

    override fun onEnabled(context: Context) {
        // Widget is placed for the first time
    }

    override fun onDisabled(context: Context) {
        // Last widget instance removed
    }

    companion object {
        internal fun updateAppWidget(
            context: Context,
            appWidgetManager: AppWidgetManager,
            appWidgetId: Int
        ) {
            // Get any data stored from Flutter via home_widget
            val widgetData = HomeWidgetPlugin.getData(context)
            val statusText = widgetData.getString("status", "Tap to record")

            // Create the RemoteViews object
            val views = RemoteViews(context.packageName, R.layout.oracy_widget)

            // Update status text if available
            views.setTextViewText(R.id.widget_status, statusText)

            // Create intent to launch app with record action
            val recordIntent = Intent(context, MainActivity::class.java).apply {
                action = WidgetRecordIntentContract.ACTION_RECORD
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                putExtra(
                    WidgetRecordIntentContract.EXTRA_RECORD_TOKEN,
                    WidgetRecordIntentTokens.getOrCreate(context)
                )
            }

            val recordPendingIntent = PendingIntent.getActivity(
                context,
                0,
                recordIntent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )

            // Set click listener on the record button
            views.setOnClickPendingIntent(R.id.widget_record_button, recordPendingIntent)

            // Also make the whole widget clickable
            val launchIntent = context.packageManager.getLaunchIntentForPackage(context.packageName)
            if (launchIntent != null) {
                val launchPendingIntent = PendingIntent.getActivity(
                    context,
                    1,
                    launchIntent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
                )
                views.setOnClickPendingIntent(R.id.widget_title, launchPendingIntent)
            }

            // Update the widget
            appWidgetManager.updateAppWidget(appWidgetId, views)
        }
    }
}
