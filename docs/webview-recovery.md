# Recovering an unresponsive window

When a Wisp window no longer accepts clicks, native recovery actions remain
available without asking the page's JavaScript to handle the click:

- **Windows:** right-click the Wisp notification-area icon. Choose **Stop agent
  in last active window** or **Reload last active window**. The target is the
  most recently focused Wisp workspace window, excluding the desktop pet. If
  that window was closed, the main window is used.
- **macOS:** use the native **Window** menu's **Stop current agent** or **Reload
  current window** action. Only the focused window handles the action.
- Linux currently has automatic recovery; these native menu entries are not
  exposed there.

Stop targets only that window's active session and uses the existing native/ACP
cancellation path. A window without an active session does nothing. Reload
recreates only that window's page; it does not restart Wisp, cancel a Run, or
terminate a remote SSH job. In-memory editor drafts and MCP App selections can
be lost on reload. Persisted sessions can be reopened, and backend agents and
the Run Manager remain alive. This does not guarantee survival of arbitrary
unmanaged remote shell processes when the entire application exits.

## Automatic recovery and evidence

Each workspace window reports a heartbeat every five seconds. The backend
tracks windows separately, checks every ten seconds, and reloads a focused
window after sixty seconds without a heartbeat. Returning from an unfocused
window grants a fresh sixty-second grace period. A window that never finishes
initializing is also covered. Attempts have a per-window two-minute cooldown,
including failed reload requests; failure to boot after a reload remains
eligible for another attempt. Closing a window removes its health state.

The log contains `webview health` once per minute per reporting window, plus
`native stop requested` and `webview recovery requested` records with a window
label and reload success/error. The heartbeat carries numeric diagnostics:

- maximum visible-page timer delay and longest observed long task;
- cumulative long-task, script-error and unhandled-rejection counts;
- active and parked MCP App counts, cumulative incoming App messages;
- current drag-overlay count.

Cumulative counters and maxima are bounded and reset with the document. No
conversation text, plugin arguments, image data, URLs or error messages from
JavaScript are included. Long-task metrics depend on browser support. A living
timer does **not** prove that clicks work: healthy heartbeats with a stuck UI
call for checking overlays, pointer targeting and script failures. A native
reload request being accepted also does not prove that navigation completed.

## Validation

Run the focused browser workload from `ui-tests`:

```powershell
$env:UI_TEST_PORT="1432"
npx playwright test tests/webview-health.spec.ts tests/long-session-stress.spec.ts tests/chat-responsive.spec.ts --workers=1
```

The mock workload combines a paginated long transcript, two simultaneous
session streams, and two App instances with 96 local thumbnail cards each.
One App remains parked while the other is visible. The test clicks a candidate,
Stop, tab close and New session, and checks numeric diagnostics. It also guards
against a model picker overflowing over Stop in a narrow chat split.

Run the optional native smoke from the repository root:

```powershell
$env:WISP_CATALOG_OFFLINE="1"
cargo run -p wisp-tauri --example webview_recovery_smoke
```

This opens two hidden, disposable **real WebView2** windows on Windows (the
platform WebView elsewhere), with temporary browser profiles and fake backend
sessions. It blocks one page's JavaScript for fifteen seconds, exercises the
same Rust stop/reload functions as the native menus, and checks that the scoped
stop completes before the block ends and that only the target page reboots.
It does not open the user's SQLite database, contact MCP/model services, or
exercise a real SSH job. Native menu clicking, macOS execution, and the exact
SFL 0.6.7 incident still require separate manual validation.

Manual Windows check: open two project windows, focus the second, then use the
tray's stop and reload entries. Check the target label in the log, the first
window's unchanged state, and existing Run records. For a stopped heartbeat,
check that another window's healthy heartbeat cannot suppress target recovery.

## Scope and follow-up

This addresses recovery gaps and the reproduced narrow-pane click obstruction
reported while investigating #1179. It does not establish the original stall's
root cause. Independent MCP App WebViews, suspending parked Apps, candidate-list
virtualization and cross-session plugin-process sharing remain separate work,
to be guided by the new diagnostics and profiling of the real SFL workload.
