# Persistence review of `2bbd848`

Scope: `crates/voiceloop-project/**`, `crates/voiceloop-engine/src/project.rs`, and `crates/voiceloop-engine/src/looper/{transfer.rs,processor.rs,mod.rs}`, reviewed against the persistence README/interface/interop documents and RT audit items #2, #3, #6, and #8.

## Confirmed defects (ranked)

### 1. High — a command queued after `Stop` can strand the looper permanently in a drain

**Files:** `crates/voiceloop-engine/src/looper/processor.rs:769-781,788-805,911-945`; the existing test at `crates/voiceloop-engine/src/looper/tests.rs:899-905` does not assert that the drain completed.

**Failure scenario:** `drain_commands` checks `self.drain` only before entering its loop. If `Stop` and `Play` are both in the command ring at the start of one callback, `Stop` creates a latency drain and sets `run = Stopped`, but the loop immediately applies `Play`, which changes `run` to `Running`. `process` only calls `drain_frame` in `Stopped | Paused`, so the drain is never cleared. Future callbacks return at line 770 without consuming commands; transport/audio may keep running while every later command is ignored. The same hole permits other commands queued behind `Stop`/`Restart` to mutate state during the supposedly exclusive drain. This is a realistic fast double-click/batched-command edge, not merely a synthetic ordering.

**Suggested fix:** after every applied command, break out of command draining as soon as `self.drain.is_some()`. Stronger: represent draining as an explicit run state and have `play`, `record`, restore, and transport mutations reject or queue while in it. Extend the test to assert `drain.is_none()` indirectly (a later command is consumed), the recording is closed, transport rewinds, and no playback occurs during the tail. Add Stop→Play, Stop→Restart, Restart→Play, and Stop-during-count-in cases where both commands are present before one callback.

### 2. High — project import is destructive before it is known to succeed

**Files:** `crates/voiceloop-engine/src/looper/processor.rs:1241-1311,1385-1396`; `crates/voiceloop-engine/src/looper/transfer.rs:421-475`; `crates/voiceloop-engine/src/project.rs:856-862`.

**Failure scenario:** `Restore` clears every current track before acquiring and filling the replacement layers. The importer then performs many asynchronous `Write` jobs. If audio stops, the device disappears, the transfer stalls, or a write fails after `Restore`, `ProjectGlue` reports failure but cannot restore the old session. Worse, dropping a core while `loading` returns without parking anything, so a route/device failure during Open or Import Audio loses the old loops and the incomplete replacement. This violates the user-visible expectation that a failed open leaves the current project intact.

**Suggested fix:** make restore transactional: reserve/fill a second set of buffers, validate every expected slot/chunk, and swap track stacks only on `Finish`. If memory policy forbids a second full set, retain the pre-restore `Captured` state and automatically roll it back before reporting failure; do not discard parked state merely because loading was active. Validate that the supplied layer list exactly matches the plan (no missing, duplicate, or out-of-range slots) before sending `Restore`.

### 3. High — lexical path checks do not contain symlink traversal

**Files:** `crates/voiceloop-project/src/doc.rs:383-417`; `crates/voiceloop-project/src/package.rs:76-98,199-220,261-283`.

**Failure scenario:** `check_relative_path` blocks `/`, drive letters, backslashes, and `..`, but `dir.join(relative)` follows symlinks. A crafted `.voiceloop` package can make `audio` (or a child beneath it) a symlink outside the package. Open/load then reads outside the package. Saving through an `audio` symlink writes content-addressed WAVs outside it, and garbage collection can delete outside files named `layer-*` or containing `.tmp-`. The JSON path remains lexically valid throughout.

**Suggested fix:** reject symlink components for the package and `audio` tree using `symlink_metadata`, and ensure each opened referenced file's canonical path remains beneath the canonical package root. For writes/deletes, use directory-relative no-follow operations where available; at minimum verify the package root and `audio` directory immediately before every mutation and refuse a symlinked target.

### 4. High — fallback selection does not validate audio, so a good previous save is bypassed

**Files:** `crates/voiceloop-project/src/package.rs:233-283`; `crates/voiceloop-project/src/recovery.rs:59-71`; `crates/voiceloop-engine/src/project.rs:690-699`.

**Failure scenario:** `open_package` accepts the current save after parsing JSON and checking only that referenced paths exist. The engine subsequently verifies WAV structure/frame count/rate/checksum in `load_audio`. If a newly referenced layer is truncated, edited, or swapped but still exists, `load_audio` fails and the engine never tries `project.previous.json`, even when that save and its older audio are valid. `recovery::find` likewise advertises a recovery slot whose audio may be unreadable. This falls short of the documented automatic fallback/corruption handling.

**Suggested fix:** choose the save only after validating and loading all referenced audio. Expose one package operation that tries current `{doc + audio}`, then previous `{doc + audio}` on any corruption/missing-file error, while preserving `UnsupportedVersion` behavior. Have recovery discovery use the same full verification.

### 5. Medium — successful saves can reuse a corrupted content-addressed layer

**Files:** `crates/voiceloop-project/src/package.rs:71-116`.

**Failure scenario:** when the expected hash-named WAV already exists, `write_layer` reuses it if its parsed frame count matches. It does not verify sample rate, channels/format, truncation beyond the reader's flag, or sample checksum. If a layer file is modified without changing its frame count, a later Save reports success and keeps the corrupt file; reopening then fails checksum validation. A PCM file can also masquerade as the expected float layer if it has the same frame count.

**Suggested fix:** before reuse, require mono float32 at the project rate and verify the decoded sample checksum (or store and verify a cryptographic/file-byte hash). If validation fails, atomically rewrite the file from the in-memory samples.

### 6. Medium — the claimed crash-safety protocol silently ignores directory-sync failures and omits the new package's parent

**Files:** `crates/voiceloop-project/src/util.rs:24-57`; `crates/voiceloop-project/src/package.rs:76-91`; `crates/voiceloop-project/src/recovery.rs:37-55`.

**Failure scenario:** `sync_dir` discards both directory-open and `sync_all` errors, yet `atomic_write`/`WavWriter::finish` return success and the documentation promises temp → fsync → rename → directory fsync. Also, creating a new package directory is not followed by an fsync of its parent, so a system crash can lose the package directory entry after Save has reported success. Recovery directory creation has the same gap.

**Suggested fix:** make directory syncing fallible and propagate failure. After creating a package/recovery directory, fsync its parent; after creating `audio`, fsync the package directory. Document any platform where durable directory sync cannot be provided instead of reporting the full guarantee.

### 7. Medium — valid public `WavSpec` values can panic or write internally inconsistent headers

**Files:** `crates/voiceloop-project/src/wav.rs:102-121,230-237,270-306`.

**Failure scenario:** validation permits every nonzero `u16` channel count through 768 kHz. `block_align` is then truncated to `u16`, while `sample_rate * block_align as u32` can overflow (and panic in debug builds). For example, high-channel 24-bit files produce a wrapped `nBlockAlign` and/or byte rate; VoiceLoop's own reader rejects the resulting header. The writer therefore does not actually support the documented “any channel count.”

**Suggested fix:** checked-compute block alignment and average bytes/sec during validation; reject specs that do not fit the RIFF/WAVE `u16`/`u32` fields. Use the checked values when constructing the header. Keep the current 24-bit little-endian three-byte packing; that part is correct.

### 8. Medium — SMF writer silently wraps timestamps above the four-byte VLQ limit

**Files:** `crates/voiceloop-project/src/smf.rs:56-71,107-134`.

**Failure scenario:** `vlq` masks every value with `0x0fff_ffff`. `SmfEvent.tick` is `u32`, so an event at `0x1000_0000` or a larger delta is accepted but encoded near tick zero. The conductor end-of-track delta is truncated the same way. Long imported/exported performances can therefore be placed at the wrong time without an error.

**Suggested fix:** make VLQ encoding return `Result` and reject any delta above `0x0fff_ffff`, or split long gaps using legal no-op/meta events. Validate every event delta and conductor EOT delta before emitting bytes.

### 9. Medium — SMF reader accepts malformed status/meta structure and can synthesize wrong notes

**Files:** `crates/voiceloop-project/src/smf.rs:294-358`.

**Failure scenario:** SysEx and meta events do not clear running status as SMF requires, channel data bytes are masked instead of being checked for `< 0x80`, a time-signature exponent that cannot fit in `u8` becomes denominator zero, tempo value zero is accepted, and a track with no EOT (or bytes after EOT) is accepted. A damaged or hostile SMF can thus be reported as successfully imported with fabricated notes, zero/invalid timing, or ignored trailing events rather than a typed corruption error.

**Suggested fix:** implement the SMF status-state rules explicitly (clear running status on SysEx and meta events), reject high-bit channel data, require nonzero tempo and a representable denominator, and require exactly one terminal EOT with no event bytes after it. Add malformed fixtures for each case.

### 10. Medium — version fields above `u32::MAX` wrap into supported versions

**Files:** `crates/voiceloop-project/src/doc.rs:428-460`.

**Failure scenario:** JSON version values are read as `u64` and narrowed with `as u32`. A document with `formatVersion: 4294967297` becomes version 1; the same wrap applies to `minReaderVersion`. A file that explicitly requires an impossibly newer reader can therefore bypass `unsupportedVersion` and be parsed under the v1 schema, undermining migration and forward-compatibility policy.

**Suggested fix:** use `u32::try_from` and return `unsupportedVersion`/`corrupt` on overflow before migration indexing or typed deserialization.

## Suspicions requiring measurement or a design decision

### S1. Realtime transfer work is bounded but its deadline claim is unverified

**Files:** `crates/voiceloop-engine/src/looper/transfer.rs:41-49`; `crates/voiceloop-engine/src/looper/processor.rs:1151-1181`.

Four 16,384-frame jobs can copy 256 KiB in one callback in addition to normal mixing, command work, and snapshots. This is allocation-free and bounded, so it is not a confirmed RT-safety violation, but the claimed “20–40 µs” is not established by the supplied tests and may be material for a 64-frame deadline on slower Macs or under memory pressure. Benchmark worst-case Read and Write callbacks on supported Intel/Apple Silicon hardware; if headroom is poor, lower `CHUNK_FRAMES`/`JOBS_PER_BLOCK` or enforce a time/work budget per callback.

### S2. No two-writer exclusion exists for package saves

**Files:** `crates/voiceloop-project/src/package.rs:119-222`; acknowledged in `docs/projects/persistence/README.md:150-152`.

Two app instances can independently rotate `project.json` to `project.previous.json`, replace the current document, and garbage-collect files based on different snapshots. Atomic per-file rename does not make that multi-file transaction safe. This is documented as PENDING rather than claimed complete, so it is not counted above, but shipping document packages should add an advisory package lock plus stale-lock recovery before multi-instance use is possible.
