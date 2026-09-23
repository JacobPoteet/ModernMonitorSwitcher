# Modern Monitor Switcher

Save your monitor arrangements as named profiles and switch between them from
the system tray, a hotkey, or the command line. Installs properly, starts with
Windows, and updates itself.

Windows 10 and 11.

## Why

Two computers, one set of monitors. When the work machine comes on, the
personal machine needs to release two of its three displays so the monitors
switch over on their own; when work is done, it takes them back. Doing that
through the Windows display settings every day is tedious, and the older tools
that automate it have to be relaunched by hand after every reboot.

This does that in one click, and stays out of the way otherwise. The second
computer needs nothing installed.

## Install

Download the installer from the
[latest release](https://github.com/JacobPoteet/ModernMonitorSwitcher/releases/latest)
and run it. It installs per-user, so there is no administrator prompt, and it
updates itself from then on.

The installer is not code-signed, so Windows SmartScreen will warn the first
time. Choose **More info** then **Run anyway**. Two ways to avoid that:

- Unblock the file first — right-click the download, **Properties**, tick
  **Unblock**. Or `Unblock-File .\ModernMonitorSwitcher_*_x64-setup.exe`.
- Or build and install from a checkout, which never involves a download at
  all: `powershell -ExecutionPolicy Bypass -File tools/install-local.ps1`.

SmartScreen only inspects files carrying a Mark-of-the-Web, which is the NTFS
stream a browser attaches to things it downloads. Either approach means there
is no such stream, so nothing warns. Updates after the first install do not
warn either: the updater writes its download directly rather than through a
browser.

It installs per-user, into `%LOCALAPPDATA%\ModernMonitorSwitcher`, rather than
into Program Files. That is deliberate: a per-user install needs no
administrator rights, which means updates can install themselves without
putting a UAC prompt in your way every time. The cost is that it is installed
for one Windows account rather than the whole machine.

## Use

Arrange your monitors however you want them using the normal Windows display
settings, then open Modern Monitor Switcher and choose **Save current layout**.
Give it a name — `Work` and `Play`, say. Repeat for each arrangement.

The first time it opens, a short guide walks through this. **Show setup
guide** in Settings brings it back.

After that, switching is one click in the tray menu.

**Start with Windows** is in the settings window. Turn it on once and the
application is simply always there.

### Hotkeys

Each profile can have a global hotkey. Click the hotkey button next to a
profile and press the combination you want. It needs at least one of Ctrl, Alt
or Shift, so you cannot accidentally take over a bare letter system-wide.

### Command line and Stream Deck

`msw.exe` is installed alongside the application, at
`%LOCALAPPDATA%\ModernMonitorSwitcher\msw.exe`.

```
msw list                 List saved profiles
msw current              Show what is on screen now
msw save "Work"          Save the current layout
msw apply "Work"         Switch to a profile
msw apply "Work" --dry-run   Check it would work, change nothing
msw delete "Work"
msw rename "Work" "Office"
msw monitors-off         Switch every monitor off
msw reset                Restore the layout Windows remembers
```

For a Stream Deck button, use the **System → Open** action with:

```
"%LOCALAPPDATA%\ModernMonitorSwitcher\msw.exe" apply "Work"
```

The command line works whether or not the tray application is running — it
talks to Windows directly, not to the app.

## If something goes wrong

**The displays end up somewhere unusable.** Run `msw reset`, or use **Restore
Windows layout** in the settings window. That asks Windows to restore its own
remembered arrangement for whatever monitors are currently connected. If you
cannot see anything at all, Windows reverts an unconfirmed display change after
about 15 seconds on its own; failing that, boot into safe mode.

**A profile will not apply.** Run `msw apply "Name" --dry-run`. It reports
whether Windows would accept the profile, and by which matching strategy,
without touching the screen. Most failures mean a monitor the profile expects
is not connected.

**Something else.** The tray application writes a log to
`%APPDATA%\ModernMonitorSwitcher\msw.log`. It is the first place to look, and
worth attaching to a bug report.

For the command line, turn logging up with an environment variable:

```
set MSW_LOG=debug
msw apply "Work"
```

The same variable raises the tray application's logging; it accepts anything
[`tracing`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
understands, so `debug` covers everything and `ModernMonitorSwitcher=debug`
covers only this application.

## How it works

Windows exposes display topology through the CCD API: `QueryDisplayConfig`
reads the current arrangement, `SetDisplayConfig` puts one back. A profile is
the captured topology, stored as JSON in
`%APPDATA%\ModernMonitorSwitcher\profiles`.

The complication is that adapter LUIDs are not stable — Windows hands out new
ones on every boot and on driver updates — so restoring a profile is not a
matter of replaying what was recorded. The saved configuration has to be
re-matched against the machine as it is now. Several strategies are tried in
order, each anchored on something that *is* stable: path ids, then monitor
device paths, then monitor names, then collapsing onto a single adapter.

Each candidate is checked with `SDC_VALIDATE` before anything is applied, so a
strategy Windows would reject never reaches the screen.

Monitor identity is only matched when it is unambiguous. Two monitors of the
same model report the same friendly name, and mapping both onto whichever was
found first would silently collapse two displays into one.

## Building

Needs [Rust](https://rustup.rs) and, for the installer, the
[Tauri CLI](https://tauri.app).

```bash
cargo test --workspace          # unit tests, no hardware needed
cargo run                       # build and launch the tray app, for working on it

# Windows ships powershell.exe (5.1); these scripts don't need pwsh (7).
powershell -ExecutionPolicy Bypass -File tools/run-sandbox.ps1    # run it with throwaway settings, beside an installed copy
powershell -ExecutionPolicy Bypass -File tools/install-local.ps1  # build the installer and install it
powershell -ExecutionPolicy Bypass -File tools/build-release.ps1  # just build the installer, without installing it
```

Work against a debug build. `target/debug/ModernMonitorSwitcher.exe` is the
same application and rebuilds in about five seconds after a change, against
roughly forty for a release build and a couple of minutes to produce an
installer. Build the installer when you are testing installing or updating,
not to try out a change.

`cargo run` uses your real settings and profiles, and quietly hands over to
an installed copy if one is running. `tools/run-sandbox.ps1` does neither: it
runs the debug build against an empty scratch folder, so it opens on the
first-run guide every time (`-Keep` carries the last run's data over). It
will not change Start with Windows or install updates. Switching profiles
still changes your real displays.

The release profile uses thin LTO rather than full. Full LTO with a single
codegen unit saved about 0.9 MB and cost two minutes on every rebuild, which
is the wrong way round for a desktop application that ships an installer over
the internet a few times a year.

The layout:

| Crate      | What it is                                                     |
| ---------- | -------------------------------------------------------------- |
| `msw-core` | The CCD API, profile storage, and the matching strategies. No UI. |
| `msw-cli`  | `msw.exe`, the command line front end.                          |
| `msw-app`  | The tray application and settings window, built with Tauri.     |

`msw-core` has no dependency on either front end, which is what keeps a future
network control surface an addition rather than a rewrite.

## Licence and credit

Mozilla Public License 2.0.

The adapter-matching strategies are ported from **MonitorSwitcher** by
**Martin Krämer**, which solved this problem first and solved it properly. See
[NOTICE.md](NOTICE.md).
