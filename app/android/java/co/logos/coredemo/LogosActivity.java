package co.logos.coredemo;

import android.app.NativeActivity;
import android.content.Intent;

/** NativeActivity, plus the logos-pair: link that opened the app while it ran:
 *  NativeActivity drops onNewIntent, so the native side polls for it here. */
public class LogosActivity extends NativeActivity {
    private static volatile String pendingLink;

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        String link = intent == null ? null : intent.getDataString();
        if (link != null) pendingLink = link;
    }

    /** The link opened since the last call, or null. */
    public static String takePendingLink() {
        String link = pendingLink;
        pendingLink = null;
        return link;
    }
}
