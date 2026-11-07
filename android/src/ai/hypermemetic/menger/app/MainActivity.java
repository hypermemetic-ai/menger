package ai.hypermemetic.menger.app;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.ApplicationInfo;
import android.graphics.Insets;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.text.InputType;
import android.util.Log;
import android.view.WindowInsets;
import android.webkit.ConsoleMessage;
import android.webkit.CookieManager;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.window.OnBackInvokedDispatcher;

/**
 * menger v2 on Android: a WebView on a menger server's {@code /app/} — the
 * server's live plexus-ui bundle (engine, components, theme), its socket and
 * its sign-in, all on the server's own origin. Nothing of menger is in the
 * APK; the server decides the version and every open page hot-swaps on a push.
 *
 * <p>The server is remembered. The launcher icon's long-press shortcuts open
 * prod or test directly, or "Switch server" picks; {@code am start -e server
 * URL} sets it from adb.
 */
public final class MainActivity extends Activity {
    static final String TAG = "menger";
    /** The two the picker offers, each a menger v2 (2026-11-06: prod runs v2 too). */
    static final String PROD = "https://app.menger.sh";
    static final String TEST = "https://app-test.menger.sh";
    static final String ACTION_SWITCH = "ai.hypermemetic.menger.app.SWITCH";
    static final String SWITCH_URL = "menger-app:switch";
    static final int PICK_FILES = 1;

    private WebView web;
    private Uri server;
    private ValueCallback<Uri[]> picking;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        FrameLayout root = new FrameLayout(this);
        web = new WebView(this);
        root.addView(web);
        setContentView(root);
        // Edge-to-edge is forced from target SDK 35: keep the page clear of
        // the system bars and the keyboard.
        if (Build.VERSION.SDK_INT >= 30) {
            root.setOnApplyWindowInsetsListener((v, insets) -> {
                Insets bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.ime() | WindowInsets.Type.displayCutout());
                v.setPadding(bars.left, bars.top, bars.right, bars.bottom);
                return WindowInsets.CONSUMED;
            });
        }
        configure();
        if (Build.VERSION.SDK_INT >= 33) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT, this::back);
        }

        String chosen = prefs().getString("server", null);
        if (chosen != null) server = Uri.parse(chosen);
        if (saved == null && handle(getIntent())) return;
        if (saved != null && server != null && web.restoreState(saved) != null) return;
        if (server == null) pick();
        else open();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handle(intent);
    }

    /** A switch (the shortcut) or a server named by the intent (adb). */
    private boolean handle(Intent intent) {
        if (intent == null) return false;
        // A page on a server (a ticket's link, a one-time sign-in link):
        // its origin becomes the server, and the page is opened as given.
        String page = intent.getStringExtra("open");
        if (page != null) {
            Uri u = origin(page);
            if (u != null && sameOrigin(Uri.parse(page.trim()), u)) {
                choose(u);
                web.loadUrl(page.trim());
                return true;
            }
        }
        String named = intent.getStringExtra("server");
        if (named != null) {
            Uri u = origin(named);
            if (u != null) {
                choose(u);
                open();
                return true;
            }
        }
        if (ACTION_SWITCH.equals(intent.getAction())) {
            pick();
            return true;
        }
        return false;
    }

    private void configure() {
        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setAllowFileAccess(false);
        s.setAllowContentAccess(false);
        s.setMediaPlaybackRequiresUserGesture(true);
        s.setUserAgentString(s.getUserAgentString() + " menger-android/" + BuildInfo.VERSION);
        CookieManager.getInstance().setAcceptCookie(true);
        WebView.setWebContentsDebuggingEnabled((getApplicationInfo().flags & ApplicationInfo.FLAG_DEBUGGABLE) != 0);

        web.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView v, WebResourceRequest r) {
                Uri u = r.getUrl();
                if (SWITCH_URL.equals(u.toString())) {
                    pick();
                    return true;
                }
                String scheme = u.getScheme();
                if (!"http".equals(scheme) && !"https".equals(scheme)) {
                    outside(u);
                    return true;
                }
                if (!r.isForMainFrame() || sameOrigin(u, server)) return false;
                // A sign-in's hops stay here (its cookies are this WebView's):
                // a redirect, a page off the server's origin, the login page,
                // a navigation no one tapped. A link tapped on the server's
                // own page to somewhere else opens in the browser.
                Uri here = v.getUrl() == null ? null : Uri.parse(v.getUrl());
                boolean onServer = here != null && sameOrigin(here, server);
                boolean onLogin = onServer && here.getPath() != null && here.getPath().startsWith("/login");
                if (r.isRedirect() || !onServer || onLogin || !r.hasGesture()) return false;
                outside(u);
                return true;
            }

            @Override
            public void onReceivedError(WebView v, WebResourceRequest r, WebResourceError e) {
                if (r.isForMainFrame()) unreachable(String.valueOf(e.getDescription()));
            }
        });

        web.setWebChromeClient(new WebChromeClient() {
            @Override
            public boolean onShowFileChooser(WebView v, ValueCallback<Uri[]> cb, FileChooserParams params) {
                if (picking != null) picking.onReceiveValue(null);
                picking = cb;
                try {
                    startActivityForResult(params.createIntent(), PICK_FILES);
                } catch (ActivityNotFoundException e) {
                    picking = null;
                    return false;
                }
                return true;
            }

            @Override
            public boolean onConsoleMessage(ConsoleMessage m) {
                Log.i(TAG, m.sourceId() + ":" + m.lineNumber() + " " + m.message());
                return true;
            }
        });

        web.setDownloadListener((url, ua, disposition, mime, length) -> outside(Uri.parse(url)));
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        if (request == PICK_FILES && picking != null) {
            picking.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(result, data));
            picking = null;
            return;
        }
        super.onActivityResult(request, result, data);
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        web.saveState(out);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        back();
    }

    private void back() {
        if (web.canGoBack()) web.goBack();
        else finish();
    }

    @Override
    protected void onDestroy() {
        web.destroy();
        super.onDestroy();
    }

    private void open() {
        web.loadUrl(server.buildUpon().path("/app/").build().toString());
    }

    private void choose(Uri u) {
        server = u;
        prefs().edit().putString("server", u.toString()).apply();
        web.clearHistory();
    }

    /** Which server: prod, test, or another address ({@link #ask}). */
    private void pick() {
        String[] urls = {PROD, TEST};
        String[] labels = {"prod \u2014 app.menger.sh", "test \u2014 app-test.menger.sh", "Other\u2026"};
        int at = 2;
        for (int i = 0; i < urls.length; i++) if (sameOrigin(Uri.parse(urls[i]), server)) at = i;
        if (server == null) at = -1;
        new AlertDialog.Builder(this)
                .setTitle("menger server")
                .setSingleChoiceItems(labels, at, (d, i) -> {
                    d.dismiss();
                    if (i < urls.length) {
                        choose(Uri.parse(urls[i]));
                        open();
                    } else {
                        ask(server != null ? server.toString() : "https://");
                    }
                })
                .setNegativeButton(server == null ? "Quit" : "Cancel", (d, w) -> {
                    if (server == null) finish();
                })
                .setCancelable(server != null)
                .show();
    }

    /** Asks for the server, prefilled with {@code current}. */
    private void ask(String current) {
        EditText field = new EditText(this);
        field.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        field.setSingleLine(true);
        field.setText(current);
        field.setSelectAllOnFocus(true);
        FrameLayout box = new FrameLayout(this);
        int pad = (int) (20 * getResources().getDisplayMetrics().density);
        box.setPadding(pad, pad / 2, pad, 0);
        box.addView(field);
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle("menger server")
                .setMessage("The address of a menger server.")
                .setView(box)
                .setPositiveButton("Open", null)
                .setNegativeButton("Back", (x, w) -> pick())
                .setCancelable(false)
                .create();
        d.setOnShowListener(x -> d.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(b -> {
            Uri u = origin(field.getText().toString());
            if (u == null) {
                field.setError("an http(s) address, e.g. " + PROD);
                return;
            }
            d.dismiss();
            choose(u);
            open();
        }));
        d.show();
    }

    private void unreachable(String why) {
        String at = server == null ? "" : server.toString();
        String html = "<!doctype html><meta name=viewport content='width=device-width,initial-scale=1'>"
                + "<style>:root{color-scheme:light dark}body{font:16px/1.5 system-ui;margin:2rem}a{display:inline-block;margin:.5rem 1rem 0 0}</style>"
                + "<h3>menger is not reachable</h3><p>" + escape(at) + "<br><small>" + escape(why) + "</small></p>"
                + "<a href='" + escape(at) + "/app/'>Retry</a><a href='" + SWITCH_URL + "'>Switch server</a>";
        web.loadDataWithBaseURL(null, html, "text/html", "utf-8", null);
    }

    private void outside(Uri u) {
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, u).addCategory(Intent.CATEGORY_BROWSABLE));
        } catch (ActivityNotFoundException e) {
            Log.w(TAG, "nothing opens " + u);
        }
    }

    private SharedPreferences prefs() {
        return getSharedPreferences("menger", MODE_PRIVATE);
    }

    /** {@code text} as an origin (scheme, host, port), or null; no scheme is https. */
    static Uri origin(String text) {
        String t = text.trim();
        if (t.isEmpty()) return null;
        if (!t.contains("://")) t = "https://" + t;
        Uri u = Uri.parse(t);
        String scheme = u.getScheme() == null ? "" : u.getScheme().toLowerCase();
        if (!scheme.equals("http") && !scheme.equals("https")) return null;
        if (u.getHost() == null || u.getHost().isEmpty()) return null;
        String auth = u.getHost().toLowerCase() + (u.getPort() > 0 ? ":" + u.getPort() : "");
        return new Uri.Builder().scheme(scheme).encodedAuthority(auth).build();
    }

    static boolean sameOrigin(Uri a, Uri b) {
        if (a == null || b == null) return false;
        return String.valueOf(a.getScheme()).equalsIgnoreCase(String.valueOf(b.getScheme()))
                && String.valueOf(a.getHost()).equalsIgnoreCase(String.valueOf(b.getHost()))
                && port(a) == port(b);
    }

    private static int port(Uri u) {
        if (u.getPort() > 0) return u.getPort();
        return "https".equalsIgnoreCase(u.getScheme()) ? 443 : 80;
    }

    private static String escape(String s) {
        return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("'", "&#39;").replace("\"", "&quot;");
    }
}
