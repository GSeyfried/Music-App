# Music App agent instructions

Personal project workspace for the Music App. Remote:
`https://github.com/GSeyfried/Music-App` (account `GSeyfried`,
`griffin.seyfried@gmail.com`).

## First reads

1. `AGENTS.md` (this file)
2. `docs/README.md`
3. `docs/projects/README.md`, then the folder for whichever project is in scope

## Workspace boundary

- All work stays inside this directory (`Music APP/`) and this repo.
- Do not create, change, move, or delete files outside this directory unless
  the user specifically authorizes it for that task. Reading outside files for
  reference is fine.
- Approval for one outside change does not carry over to the next.

## Session setup

- Run `npm start` at the start of a session. It switches git, GitHub CLI,
  Firebase CLI, and gcloud to the personal account and points `origin` at the
  Music-App repo. `npm run start:check` verifies without changing anything.
- Script: `scripts/start-project-session.mjs`. Set the `projectId` fields there
  once a Firebase/GCP project exists.

## Safety

- Do not commit, push, deploy, or create/modify cloud resources unless the user
  asks.
- Never commit credentials, tokens, or `.env` files.
- Confirm the active GitHub account is `GSeyfried` before any push.

## Documentation

- General docs live in `docs/`.
- Each project or feature area we build out gets its own folder under
  `docs/projects/<project-name>/` (see `docs/projects/README.md`).
- Record open questions as `PENDING` rather than guessing.
