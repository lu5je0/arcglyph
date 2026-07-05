// Written by ges daemon on startup and loaded into kwin via
// org.kde.KWin.Scripting DBus. Emits the focused window's resourceClass
// and fullscreen state to a well-known file whenever the active window
// changes.

function write(state) {
    var out = state.appId + "\n" + (state.fullscreen ? "1" : "0") + "\n";
    // KWin's JS engine exposes a very small stdlib. Use callDBus back into
    // our own service if we had one; instead write via a subprocess.
    callDBus(
        "org.kde.KWin",
        "/Scripting",
        "org.freedesktop.DBus.Peer",
        "Ping"
    );
    // No fs access from KWin JS. Fall back to notifying our daemon over DBus.
    // The daemon owns the well-known name at runtime.
    callDBus(
        "com.github.lu5je0.ges",
        "/focus",
        "com.github.lu5je0.ges.Focus",
        "SetActive",
        state.appId,
        state.fullscreen
    );
}

function snapshot(w) {
    if (!w) return { appId: "", fullscreen: false };
    return {
        appId: (w.resourceClass || "").toString(),
        fullscreen: !!w.fullScreen,
    };
}

function publish() {
    write(snapshot(workspace.activeWindow));
}

workspace.windowActivated.connect(publish);
if (workspace.activeWindow) {
    workspace.activeWindow.fullScreenChanged.connect(publish);
}
publish();
