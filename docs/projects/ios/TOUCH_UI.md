# Touch UI guidelines (iPadOS and iPhone, in WKWebView)

For the frontend agent. The React UI runs in a WKWebView inside the Tauri iOS
app. These rules keep one codebase working with a mouse on macOS and with
fingers, pencils, hardware keyboards, and MIDI controllers on iPad and iPhone.
Items marked *(verify)* need a device.

## 1. Principles

1. **Every action has a visible touch control.** Keyboard shortcuts
   (`src/lib/shortcuts.ts`) and MIDI mappings are accelerators, never the only way.
2. **Performance controls act on touch-down, not click.** Record, overdub, stop
   take, and play/stop fire on `pointerdown` (with visual feedback in the same
   frame), because musicians press on the beat. Destructive actions (clear,
   delete) use press-and-hold or a confirm step, never a single tap.
3. **No hover-only information.** No tooltips as the only label, and no controls
   that appear on hover. Gate hover styles with `@media (hover: hover)` so they do
   not stick after a tap.
4. **Audio time is authoritative** (ARCHITECTURE rule 5). Visuals interpolate
   from the latest engine snapshot plus elapsed time, so they stay correct at
   30, 60, or 120 Hz.

## 2. Sizes and layout

| Element | Minimum | Recommended |
| --- | --- | --- |
| Any tappable control | 44 × 44 pt (Apple HIG) | 48 pt or more, 8 pt gaps |
| Record / overdub (per track) | 64 pt | 72-96 pt on iPad, full-width bar on iPhone |
| Transport (play/stop) | 56 pt | Thumb-reachable bottom area on iPhone |
| Faders / gain | 44 pt wide track | Vertical drag with fine mode (second finger or long-press) |

Layout widths to support (CSS px = pt in WKWebView at default zoom):

| Context | Width (pt) | Layout |
| --- | --- | --- |
| iPhone portrait | 375-440 | One column. Tracks as a vertical list. Transport bar pinned to the bottom. Practice Lab as its own tab. |
| iPhone landscape | 667-956 (minus notch insets) | Two columns are possible; short height (about 375-440), so no tall meters. |
| iPad Split View ⅓ / Slide Over | about 320-375 | Same as iPhone portrait. **Must work.** |
| iPad Split View ½ | about 507-694 | Compact two-pane. |
| iPad full screen | 744-1376 | Full Studio (current desktop layout). |
| Stage Manager | Any size, resizable | Container queries, not device detection. |

- Use **container queries** (`@container`) and `clamp()` on the components,
  not `navigator.userAgent` or fixed breakpoints only. The current
  `@media (max-width: 860px)` single-column switch is a reasonable start.
- Ignore `tauri.conf.json` `minWidth: 720`. It does not apply on iOS, and the
  UI must handle 320 pt.
- Orientation changes and Split View resizes happen live. Canvases must resize
  (ResizeObserver) and redraw with the new DPR.

## 3. Safe areas, viewport, and scrolling

Change `index.html` viewport meta to:

```html
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no, viewport-fit=cover" />
```

- `viewport-fit=cover` + padding from `env(safe-area-inset-top|right|bottom|left)`
  on the app shell. The home indicator needs bottom inset under the transport
  bar, and the notch/Dynamic Island needs side insets in landscape.
  WKWebView inset quirks exist on iOS 26 after keyboard show/hide
  ([Apple forum 802159](https://developer.apple.com/forums/thread/802159)), so do not rely on
  insets being correct on first paint *(verify)*.
- **Disable page bounce and pull-to-refresh-style overscroll**: make the app shell
  `position: fixed; inset: 0; overflow: hidden`, and scroll only inner panes with
  `overflow: auto; overscroll-behavior: contain`. Also add `html, body { overscroll-behavior: none; }`.
  If bounce persists, the native fix is `webView.scrollView.bounces = false`,
  which needs a small Swift/objc2 hook *(verify)*.
- `-webkit-text-size-adjust: 100%` so rotation does not rescale fonts.

## 4. Gestures and WKWebView pitfalls

| Pitfall | Fix |
| --- | --- |
| Double-tap zoom / historical 300 ms tap delay | `width=device-width` already removes the delay. Add `touch-action: manipulation` globally and `touch-action: none` on custom controls (pads, faders, waveform scrubbing). |
| Pinch zoom of the whole UI | `user-scalable=no, maximum-scale=1` (WKWebView honours this by default). For pinch inside a waveform, handle two pointers ourselves with `touch-action: none` on that element. |
| Long-press shows a text selection loupe or callout | `-webkit-user-select: none; user-select: none; -webkit-touch-callout: none;` on the app shell. Re-enable selection only in text inputs and notes. |
| Grey tap flash | `-webkit-tap-highlight-color: transparent;` and provide our own `:active` and pressed state. |
| Context menu | No right-click on touch. Use long-press (≥ 500 ms) with a visible menu button as the alternative. Prevent the `contextmenu` default on controls. |
| Mouse events emulated from touch | Use **Pointer Events** only (`pointerdown/move/up/cancel`). Do not mix with `touchstart`/`click`, or actions double-fire. |
| Lost drags when a finger leaves the control | `element.setPointerCapture(e.pointerId)` on `pointerdown`. Handle `pointercancel` (the system can steal the touch, e.g. a Control Center swipe). |
| iOS edge gestures | Keep critical controls 16-20 pt away from screen edges. Bottom-edge swipes go home and top-right opens Control Center. Swipe navigation between workspaces should start inside the content, not at the edge. |
| Virtual keyboard covers inputs | Few text inputs. Use `visualViewport` resize to keep the focused field visible. Numeric fields (BPM) use `inputmode="decimal"` or, better, steppers and drag. |
| `alert` / `confirm` | Avoid. Use in-app sheets (consistent across platforms). |

## 5. Multitouch

- Track pointers by `pointerId`. Two simultaneous presses (e.g. record track 1
  while muting track 2, or two-finger fader moves) must both work. Do not
  keep a single "active control" global.
- Each control owns its pointer from down to up and ignores others.
- Send one engine command per gesture start or end, not per `pointermove`.
  Throttle continuous controls (gain) to ≤ 30 Hz and send the final value on `pointerup`.
- Apple Pencil arrives as `pointerType === "pen"`. Treat it like touch, and
  optionally use pressure for waveform editing later.

## 6. Navigation

- Workspaces (Studio / Practice Lab): bottom tab bar on iPhone, top segmented
  control or sidebar on iPad. Optional horizontal swipe between them, but **never
  on areas that also take horizontal drags** (waveforms, faders, loop progress).
- Settings and diagnostics: bottom sheet on iPhone, popover or sidebar on iPad.
- Keep the transport and the selected track's record button visible in every
  layout. They are the instrument.

## 7. Rendering budget (canvas and rAF)

- WKWebView `requestAnimationFrame` runs at **60 Hz max** even on 120 Hz
  ProMotion iPads, **30 Hz in Low Power Mode**, and **not at all in the background**
  ([WebKit explainer](https://webkit.github.io/explainers/animation-frame-rate),
  [bug 215745](https://bugs.webkit.org/show_bug.cgi?id=215745),
  [bug 294338](https://bugs.webkit.org/show_bug.cgi?id=294338)).
  `useFrame.ts` already shares one loop. Draw from `now` (elapsed time) and the
  latest snapshot, never "one step per frame".
- Engine events arrive at ~30 Hz. Interpolate playheads between snapshots using
  `sampleRate` and the frame counter.
- Canvas: cap the backing store at DPR 2 (iPhones are DPR 3), size canvases to
  their CSS box, and avoid many full-screen canvases. iOS WebKit has a total canvas
  memory limit and silently fails large allocations *(verify)*.
- Prefer CSS transforms (`transform`, `opacity`) for meters and playheads over
  layout-changing properties.
- On return from the background, drop stale animation state and redraw from the
  current snapshot. Do not replay missed events.

## 8. Hardware keyboard (iPad)

- `keydown` from hardware keyboards reaches the webview. Keep `shortcuts.ts` as is.
  Ignore events while focus is in a text field. Call `preventDefault()` on handled
  keys (Space would otherwise scroll or activate the focused button).
- iPadOS reserves several ⌘ combinations (⌘H, ⌘Tab, ⌘Space). Our shortcuts are
  unmodified keys, which is good.
- Show the shortcut sheet ("?") on iPad only when a hardware keyboard has been
  seen (first `keydown`). There is no reliable API to detect one ahead of time.
- Later (nice to have): native `UIKeyCommand`s via a Swift plugin, so shortcuts appear
  in the iPadOS hold-⌘ overlay.

## 9. MIDI controllers and foot pedals

- Plan a "MIDI learn" mapping layer, rendered as UI here, with mapping stored in the
  project or app settings: CC or note → action ID (the same `ShortcutId` set
  plus per-track targets). Reuse the shortcut action table so keyboard,
  MIDI, and touch call the same functions.
- Feedback: light up the mapped control on incoming MIDI so users can see the mapping work.

## 10. Audio-related UI behaviour on iOS

- **Permission states**: show a dedicated empty state for "microphone access is off"
  with the Settings path. iOS cannot deep-link to the mic toggle; `app-settings:`
  opens the app's Settings page (needs a native hook) *(verify)*.
- **Route changes**: when the engine reports a route change, show which input
  and output are active (for example "H4n Pro → Headphones"). Show a warning chip for
  Bluetooth output latency.
- **Interruptions**: on `audioSession: interrupted`, show a paused overlay with
  "Resume". Never auto-resume recording.
- **Input monitoring**: offer it only when the output route is headphones or a
  line-out (feedback risk on built-in speakers).
- No Web Audio, `getUserMedia`, or `<audio>` elements. All audio is in the Rust
  engine, so WebKit autoplay and gesture-unlock rules do not apply. Keep it that way.

## 11. Accessibility

- Real `<button>` elements with `aria-label`, and pressed state via `aria-pressed`.
  VoiceOver on iPad works with standard elements.
- Respect `prefers-reduced-motion` (disable decorative animation, keep playheads).
- Support Dynamic Type loosely: use `rem` and a root size from
  `-apple-system-body` font *(verify)*, and check layouts at a larger text size.
- Colour is never the only state indicator (record = red **and** a filled icon or label).

## 12. Checklist for each new component

- [ ] Works at 320 pt wide and in landscape iPhone height.
- [ ] Hit targets ≥ 44 pt. Performance controls fire on `pointerdown`.
- [ ] Pointer Events only. Handles `pointercancel`. Multitouch-safe.
- [ ] No hover-only affordance. No right-click-only action.
- [ ] `touch-action` set appropriately. No text selection or callout.
- [ ] Visuals time-based, correct at 30 Hz.
- [ ] Keyboard and MIDI action mapped through the shared action table.
