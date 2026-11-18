# Interaction design direction

Status: user-directed product requirements, not yet fully implemented.

VoiceLoop is expected to grow from a looper into an advanced music workstation.
Its interaction system must scale to detailed editing without making the live
performance surface feel dense. Desktop power interactions are encouraged,
with visible and touch-accessible equivalents on iPhone and iPad.

## Principles

1. **Fast path plus discoverable path.** Right-click, double-click, keyboard
   shortcuts, and gestures are fast paths. Every action also has a visible,
   keyboard-accessible, and touch-accessible route.
2. **Keep performance controls immediate.** Record, overdub, transport, mute,
   and solo stay one action away. Detailed editing opens without crowding the
   performance view.
3. **Progressive disclosure.** Start with the musical essentials, then expose
   deeper controls through inspectors, context menus, modals, drawers, and
   dedicated detail views.
4. **One command system, many inputs.** Buttons, menu items, shortcuts,
   gestures, MIDI mappings, and accessibility actions should invoke stable
   command IDs rather than duplicate business logic.
5. **Platform-native adaptation.** macOS, iPad, and iPhone share concepts and
   project data, but navigation and input affordances adapt to the device.

## Desktop power interactions

### Context menus

Right-click context menus are a preferred quick-action surface throughout the
app. Likely targets include tracks, layers/takes, clips, timeline selections,
effect slots, devices, references, and practice attempts.

- Put frequent contextual actions first and group destructive actions last.
- Include relevant shortcut hints and current state (checked/toggled items).
- Support keyboard invocation (`Shift+F10` or the context-menu key) and an
  explicit overflow/menu button for discoverability.
- On touch, expose the same command set through long-press or an overflow
  button; do not make right-click the only route.

### Double-click and detail views

Double-click should open the natural detailed view for an element: a track
inspector/editor, layer or clip editor, effect editor, device details, or a
practice result breakdown. The same destination must also be reachable through
an **Open**, **Edit**, or **Details** action.

Use the presentation that fits the task:

- Inspector/sidebar for adjustments that benefit from seeing the session.
- Modal for focused, bounded configuration or confirmation.
- Drawer/bottom sheet for compact or touch layouts.
- Full editor route for waveform, pitch, automation, or arrangement work.

Double-click must not trigger a conflicting single-click action twice. Select
on single click; open details on double-click unless a component explicitly
documents another behavior.

### Hover explanations and examples

Mac controls should provide concise hover explanations, including examples or
small visual/audio previews when they materially teach a feature.

- Tooltips appear after a short delay and never obstruct performance controls.
- Do not autoplay audio on hover. A preview must require an explicit press and
  have an obvious stop action.
- The same explanation must be available on keyboard focus and through an info
  action or help mode on touch devices.
- Tooltips supplement labels; essential meaning never exists only on hover.

### Trackpad gestures

Design for Mac trackpads and iPad trackpads without overriding system gestures:

- Two-finger scroll pans scrollable tracks and timelines.
- Pinch zooms waveforms, timelines, pitch views, and automation lanes—not the
  entire application chrome.
- Horizontal swipes may navigate history or adjacent detail panels only when
  they cannot be confused with timeline editing.
- Fine adjustment can use a modifier key, a secondary finger, or an explicit
  fine-control mode.
- Gesture handling must be cancelable, tested at gesture boundaries, and never
  fire destructive or recording actions accidentally.

## Navigation that scales

Use a clear hierarchy as functionality grows:

1. **Primary navigation:** Studio, Practice Lab, projects/library, and other
   major workspaces.
2. **Workspace sub-navigation:** for example Live, Arrange, Mix, Edit, or
   References/Attempts/Results.
3. **Contextual navigation:** selected track, clip, effect, or analysis detail.

On wide screens this can be a top navigation bar plus a workspace sub-bar and
an optional inspector. On iPhone, the same hierarchy should use a compact tab
bar, navigation stack, and sheets rather than squeezing multiple bars across
the screen. Preserve selection and scroll position when moving between views.

Avoid arbitrarily deep navigation. Detail views need a clear title, Back/Done
action, and—where useful—breadcrumbs or a visible statement of what is being
edited.

## Rebindable commands, hotkeys, and gestures

Build a command registry before shortcut count grows substantially. Each
command should have:

- Stable ID, label, category, description, and availability predicate.
- Default bindings by platform and form factor.
- Zero or more keyboard, pointer/gesture, MIDI, and accessibility bindings.
- Conflict detection, reserved-system-shortcut protection, reset-to-default,
  import/export, and searchable command-palette representation.
- User overrides persisted per profile and, when appropriate, per device.

Recommended profiles:

- macOS keyboard + trackpad/mouse.
- iPad touch + optional keyboard/trackpad/Pencil.
- iPhone touch, with fewer simultaneous controls and gesture defaults chosen
  conservatively.

A gesture binding must have a textual description and a non-gesture
alternative. Rebinding should not allow the user to remove the only accessible
path to essential safety actions such as stopping playback or recording.

## Cross-platform interaction map

| Intent | macOS | iPadOS | iPhone |
| --- | --- | --- | --- |
| Quick actions | Right-click / overflow | Long-press / trackpad right-click / overflow | Long-press / overflow |
| Open details | Double-click / Enter / Details | Double-tap where safe / Enter / Details | Tap Details or disclosure; double-tap only where unambiguous |
| Explanation | Hover or keyboard focus | Pointer hover, focus, info/help action | Info/help action or contextual sheet |
| Fine adjustment | Modifier + drag, trackpad | Pencil/trackpad, second-finger or fine mode | Long-press/fine mode plus large control |
| Navigate hierarchy | Nav + sub-nav + inspector | Adaptive sidebar/tab bar + sheets | Tab bar + navigation stack + sheets |
| Commands | Rebindable keys, gestures, MIDI | Touch plus optional rebindable keys/gestures/MIDI | Touch-first; limited safe gestures and MIDI |

## PENDING design work

- Define the initial command registry and persistence format.
- Inventory context-menu actions for every selectable domain object.
- Define gesture conflict rules for timeline/waveform editing.
- Prototype the primary/sub-navigation hierarchy with Studio and Practice Lab.
- Decide which detail surfaces are inspectors, modals, sheets, or full routes.
- Test right-click, keyboard context menus, VoiceOver, trackpad, touch, and
  Pencil behavior on actual devices.
