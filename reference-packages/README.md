# Reference packages

Source repositories downloaded for **technical evaluation and architectural
reference only**. They are **not** application dependencies, are never
compiled into the app, and are **git-ignored**. Only this README and
`manifest.json` are tracked.

The assessment is in [`docs/REFERENCE_PACKAGE_ASSESSMENT.md`](../docs/REFERENCE_PACKAGE_ASSESSMENT.md).

## Commands

```sh
npm run refs:fetch            # clone any missing repo at its pinned SHA (shallow, no submodules)
npm run refs:fetch -- juce    # just one
npm run refs:verify           # check every repo is checked out at the pinned SHA
npm run refs:status           # name / SHA / licence files
npm run refs:lock             # after intentionally updating a repo: record new SHAs into manifest.json
```

To update a reference to upstream HEAD: delete its folder, set its `sha` to
`null` in `manifest.json`, run `refs:fetch -- <name>`, then `refs:lock`.

## Retrieval policy

- Shallow (`--depth 1`) clones, **no submodules**.
  - `tracktion_engine` declares JUCE as a submodule (`modules/juce`, SSH URL).
    It was skipped; the standalone `juce` clone serves as the reference. To
    build Tracktion for real:
    `git -C reference-packages/tracktion_engine submodule update --init --depth 1`
    (requires GitHub SSH access, or rewrite the URL to HTTPS).
- **No model weights or datasets.** `rnnoise/download_model.sh` was not run.
- `signalsmith-stretch` depends on `signalsmith-linear` (not retrieved).
- `node_modules` and other package installs inside references are not performed.

## Licences: read before copying anything

Every repo keeps its original `LICENSE`/`COPYING`/`NOTICE` files. Copyleft
references must not be copied **or translated** into our code:

| Repo | Licence | Rule |
| --- | --- | --- |
| tracktion_engine | GPL-3.0+ / commercial | Design reference only |
| juce | AGPL-3.0 / commercial JUCE 9 EULA | Design reference only |
| mloop | AGPL-3.0 | Behaviour reference only |
| essentia.js | AGPL-3.0 | Algorithm survey only |

All others are permissive (MIT, Apache-2.0, BSD-3, 0BSD). Reuse requires
keeping their notices and recording the reuse in `docs/DECISIONS.md`.
