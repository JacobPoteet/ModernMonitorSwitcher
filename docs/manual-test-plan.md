# Manual test plan

Most of this project is covered by `cargo test`, but the part that matters
most cannot be: actually reconfiguring displays. `SetDisplayConfig` changes
the screen you are reading, and the interesting failure — restoring a profile
after a reboot has renumbered the adapters — needs a reboot to reproduce.

This is the checklist for a release candidate. Work through it on real
hardware with the monitors you actually use.

## Before you start

Know the escape hatch. If a profile leaves the displays unusable:

- Windows reverts an unconfirmed display change on its own after about 15
  seconds. Wait it out.
- Otherwise, blind-run `msw reset` from Win+R.
- Failing that, boot into safe mode.

## 1. Installation

- [ ] The installer runs without an administrator prompt.
- [ ] SmartScreen warns, and **More info → Run anyway** gets past it.
- [ ] The application appears in the tray after installing.
- [ ] `%LOCALAPPDATA%\ModernMonitorSwitcher\msw.exe` exists.
- [ ] The tray icon is legible against the taskbar, in both light and dark
      Windows themes.

### First-run guide

On a machine with no `%APPDATA%\ModernMonitorSwitcher` folder:

- [ ] The settings window opens on the guide.
- [ ] **Open display settings** opens the Display page of Windows Settings.
- [ ] Disconnecting a display in Windows updates the guide's preview within
      a couple of seconds, without clicking back into the window.
- [ ] Saving from the guide creates the profile and shows the final page;
      pressing Enter to save does not skip past it.
- [ ] After **Done** or **Skip**, reopening the window does not show the
      guide again. **Show guide** in Settings brings it back.
- [ ] Upgrading an install that already has profiles does not show the
      guide.

## 2. Capturing profiles

With every monitor on:

- [ ] **Save current layout** captures a profile; its tile draws the same
      arrangement as Windows' Display settings, with a taskbar strip on the
      main display.
- [ ] When the layout on screen matches no profile, a dashed **Current
      layout** tile appears first, and its **Save** opens the save dialog.
- [ ] **Save current layout > New profile...** in the tray opens the window
      straight onto the save dialog.
- [ ] Two monitors of the same model are distinguished, as `NAME #1` and
      `NAME #2`, rather than listed twice identically.
- [ ] Saving over an existing profile asks first.
- [ ] **Replace with current layout** in a profile's ... menu asks, then
      updates that profile's picture.
- [ ] A name containing `\ / : * ? " < > |` is rejected with a clear message.

With the second computer holding some of the monitors:

- [ ] A profile captured in that state records the reduced set.

## 3. Switching

- [ ] Switching from the tray works, and the displays end up as saved:
      resolution, refresh rate, arrangement, and which one is primary.
- [ ] Clicking a profile tile switches to it; its screens light up in turn.
- [ ] Right-clicking a tile opens the same menu as its ... button, and the
      menu works with the arrow keys and Escape.
- [ ] The check mark in the tray, and the highlighted **On screen** tile in
      the window, follow the profile that is actually on screen, including
      after switching from the tray or a hotkey with the window open.
- [ ] The tray tooltip names the current profile.
- [ ] Switching to the profile that is already active is a no-op, not an
      error.
- [ ] `msw apply "Name"` works from a command prompt.
- [ ] `msw apply "Name"` works with the tray application **not** running.
- [ ] `msw apply "Name" --dry-run` reports without changing anything.

### The actual workflow

- [ ] Work computer on, switch to the work profile: the two shared monitors
      go dark and hand over to the work machine.
- [ ] Work computer off, switch to the play profile: all three come back,
      arranged as before.
- [ ] Repeat a few times. It should behave identically each time.

## 4. Across a reboot

This is the case the adapter-matching strategies exist for, and the one the
unit tests can only simulate.

- [ ] Reboot.
- [ ] Switch to each profile. Everything should still work.
- [ ] Run `msw apply "Name" --dry-run -v` and check which strategy it reports.
      "as saved" means the adapter identifiers happened not to change;
      anything else means the remapping did its job.

Worth repeating after a graphics driver update, which also renumbers
adapters.

## 5. Hotkeys

- [ ] A hotkey can be assigned, and switches profiles from any application.
- [ ] A combination without Ctrl, Alt or Shift is refused.
- [ ] A combination already assigned to another profile is refused.
- [ ] Removing a hotkey releases it.
- [ ] Deleting a profile releases its hotkey.
- [ ] Renaming a profile keeps its hotkey.
- [ ] Hotkeys still work after restarting the application.

## 6. Start with Windows

- [ ] Enabling it adds the entry; a reboot brings the application back.
- [ ] After a login start, it is in the tray with **no window showing**.
- [ ] Disabling it removes the entry, and it no longer starts.

## 7. Updating

Needs two releases to test properly.

- [ ] **Check for updates now** on the current version reports no update.
- [ ] After publishing a newer tag, the installed copy offers it.
- [ ] Accepting installs and restarts into the new version.
- [ ] Declining leaves the application running normally.
- [ ] Profiles, hotkeys and the autostart setting all survive the update.
- [ ] An update signed with the wrong key is refused. (Optional, but it is the
      only way to confirm signature verification is really on.)

## 8. Edge cases

- [ ] **Turn off displays** blanks them; a key press wakes them.
- [ ] **Restore the Windows layout** recovers a sane configuration.
- [ ] Deleting the profiles folder while running does not crash it; the tray
      reports no profiles.
- [ ] Hand-editing a profile JSON into something invalid makes that one
      profile disappear from the list, and the others still work.
- [ ] Applying a profile whose monitors are not connected fails with a
      readable message rather than doing something destructive.
- [ ] Unplugging a monitor while the application runs does not crash it.

## 9. Window

- [ ] On Windows 11 the window has the Mica backdrop; on Windows 10 it is
      solid, not see-through.
- [ ] The title bar drags the window, double-click maximizes, and dragging
      it to a screen edge snaps.
- [ ] Minimize, maximize/restore and close work; close hides to the tray
      rather than quitting. The maximize glyph changes when maximized.
- [ ] The window resizes from every edge, and down to its minimum size the
      tiles reflow without overlapping.
- [ ] Light and dark mode both follow the Windows setting, including when it
      changes with the window open.
- [ ] With **Reduce animation** on in Windows, switching does not animate.
- [ ] Every control is reachable with Tab and shows a focus ring; dialogs keep
      Tab inside them, and Escape closes the top one.
