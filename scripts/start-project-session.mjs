import { spawnSync } from "node:child_process";

// Credential session for the Music App personal project.
// Switches git identity, GitHub CLI, Firebase CLI and gcloud to the personal account.
// Usage: npm start            (switch + authenticate)
//        npm run start:check  (verify only, changes nothing)

const project = {
  label: "Music App",
  git: {
    name: "Griffin Seyfried",
    email: "griffin.seyfried@gmail.com",
    githubLogin: "GSeyfried",
    origin: "https://github.com/GSeyfried/Music-App.git",
    defaultBranch: "main",
  },
  firebase: {
    account: "griffin.seyfried@gmail.com",
    // Set once a Firebase project exists; null skips `firebase use`.
    projectId: null,
  },
  gcloud: {
    configuration: "music-app",
    account: "griffin.seyfried@gmail.com",
    // Set once a GCP project exists; null skips project selection and ADC quota project.
    projectId: null,
  },
};

const checkOnly = process.argv.includes("--check");
const failures = [];
const notes = [];
const windows = process.platform === "win32";

function invoke(command, args, { quiet = false, capture = false } = {}) {
  return spawnSync(command, args, {
    cwd: process.cwd(),
    encoding: "utf8",
    shell: windows,
    stdio: quiet ? "ignore" : (capture ? "pipe" : "inherit"),
    timeout: checkOnly ? 30_000 : 600_000,
  });
}

function execute(command, args, options = {}) {
  return invoke(command, args, options).status === 0;
}

function capture(command, args) {
  const result = invoke(command, args, { capture: true });
  return {
    ok: result.status === 0,
    output: `${result.stdout ?? ""}`.trim(),
    diagnostic: `${result.stderr ?? ""}`.trim(),
  };
}

function commandExists(command) {
  return execute(command, ["--version"], { quiet: true });
}

function recovery(area, commands) {
  failures.push({ area, commands });
}

function requireCommand(command, installCommand) {
  if (commandExists(command)) return true;
  recovery(`${command} is not installed or not on PATH`, [installCommand]);
  return false;
}

function configureGit() {
  console.log("\n[Git] Configuring repository identity and GitHub account...");
  if (!requireCommand("git", "Install Git for Windows, then reopen the terminal.")) return;

  let inRepo = execute("git", ["rev-parse", "--is-inside-work-tree"], { quiet: true });
  if (!inRepo && !checkOnly) {
    console.log(`[Git] No repository yet; initializing with branch ${project.git.defaultBranch}...`);
    inRepo = execute("git", ["init", "-b", project.git.defaultBranch], { quiet: true });
  }
  if (!inRepo) {
    recovery("This folder is not a Git repository", [
      `git init -b ${project.git.defaultBranch}`,
      `git remote add origin ${project.git.origin}`,
    ]);
    return;
  }

  if (!checkOnly) {
    execute("git", ["config", "--local", "user.name", project.git.name], { quiet: true });
    execute("git", ["config", "--local", "user.email", project.git.email], { quiet: true });
    execute("git", ["config", "--local", "user.useConfigOnly", "true"], { quiet: true });
  }

  const gitName = capture("git", ["config", "--local", "--get", "user.name"]);
  const gitEmail = capture("git", ["config", "--local", "--get", "user.email"]);
  const gitUseConfigOnly = capture("git", ["config", "--local", "--get", "user.useConfigOnly"]);
  const identityReady = gitName.ok
    && gitName.output === project.git.name
    && gitEmail.ok
    && gitEmail.output.toLowerCase() === project.git.email.toLowerCase()
    && gitUseConfigOnly.ok
    && gitUseConfigOnly.output.toLowerCase() === "true";
  if (!identityReady) {
    recovery("Repository-local Git identity does not match this project", [
      `git config --local user.name "${project.git.name}"`,
      `git config --local user.email "${project.git.email}"`,
      "git config --local user.useConfigOnly true",
    ]);
  }

  let origin = capture("git", ["remote", "get-url", "origin"]);
  if (!checkOnly && origin.output !== project.git.origin) {
    const verb = origin.ok ? "set-url" : "add";
    execute("git", ["remote", verb, "origin", project.git.origin], { quiet: true });
    origin = capture("git", ["remote", "get-url", "origin"]);
  }
  const originReady = origin.ok && origin.output === project.git.origin;
  if (!originReady) {
    recovery("Git origin does not match this project", [
      origin.ok
        ? `git remote set-url origin ${project.git.origin}`
        : `git remote add origin ${project.git.origin}`,
    ]);
  }

  if (!requireCommand("gh", "winget install --id GitHub.cli")) return;
  let switched;
  if (checkOnly) {
    const active = capture("gh", ["api", "user", "--jq", ".login"]);
    switched = active.ok
      && active.output.toLowerCase() === project.git.githubLogin.toLowerCase();
  } else {
    switched = execute(
      "gh",
      ["auth", "switch", "--hostname", "github.com", "--user", project.git.githubLogin],
      { quiet: true },
    );
  }
  if (!switched && !checkOnly) {
    console.log(`[GitHub] ${project.git.githubLogin} needs authentication; opening browser login...`);
    execute("gh", ["auth", "login", "--hostname", "github.com", "--web", "--git-protocol", "https"]);
    switched = execute(
      "gh",
      ["auth", "switch", "--hostname", "github.com", "--user", project.git.githubLogin],
      { quiet: true },
    );
  }

  const active = switched ? capture("gh", ["api", "user", "--jq", ".login"]) : { ok: false };
  const verified = active.ok
    && active.output.toLowerCase() === project.git.githubLogin.toLowerCase();
  if (!verified) {
    recovery("GitHub authentication is missing, expired, or using the wrong account", [
      "gh auth login --hostname github.com --web --git-protocol https",
      `gh auth switch --hostname github.com --user ${project.git.githubLogin}`,
      "gh auth setup-git",
    ]);
    return;
  }
  if (!checkOnly) execute("gh", ["auth", "setup-git"], { quiet: true });

  // Confirm the active account can actually reach the remote repo.
  const remoteReachable = execute("git", ["ls-remote", "--exit-code", "origin"], { quiet: true })
    // An empty repo has no refs, so ls-remote exits 2; fall back to the API.
    || execute("gh", ["api", `repos/${repoSlug()}`, "--silent"], { quiet: true });
  if (!remoteReachable) {
    recovery(`${project.git.githubLogin} cannot reach ${project.git.origin}`, [
      `gh repo view ${repoSlug()}`,
      "gh auth refresh --hostname github.com --scopes repo",
    ]);
    return;
  }

  if (identityReady && originReady) {
    console.log(`[Git] Ready as ${project.git.name} / ${project.git.githubLogin} -> ${repoSlug()}.`);
  }
}

function repoSlug() {
  return project.git.origin.replace(/^https:\/\/github\.com\//, "").replace(/\.git$/, "");
}

function configureFirebase() {
  console.log("\n[Firebase] Selecting account...");
  if (!commandExists("firebase")) {
    notes.push("Firebase CLI not installed; skipped (npm install --global firebase-tools).");
    return;
  }

  let accountReady;
  if (checkOnly) {
    const accounts = capture("firebase", ["login:list"]);
    accountReady = accounts.ok
      && accounts.output.toLowerCase().includes(`logged in as ${project.firebase.account}`);
  } else {
    const selection = capture("firebase", ["login:use", project.firebase.account]);
    accountReady = selection.ok || selection.output.includes("Already using account");
  }
  if (!accountReady && !checkOnly) {
    console.log(`[Firebase] ${project.firebase.account} needs authentication; opening browser login...`);
    execute("firebase", ["login:add"]);
    const selection = capture("firebase", ["login:use", project.firebase.account]);
    accountReady = selection.ok || selection.output.includes("Already using account");
  }
  if (!accountReady) {
    recovery("Firebase authentication failed", [
      "firebase login:add",
      `firebase login:use ${project.firebase.account}`,
    ]);
    return;
  }

  if (!project.firebase.projectId) {
    console.log(`[Firebase] Ready as ${project.firebase.account} (no project configured yet).`);
    return;
  }

  let projectReady;
  if (checkOnly) {
    const selectedProject = capture("firebase", ["use"]);
    projectReady = selectedProject.ok
      && selectedProject.output.toLowerCase().includes(project.firebase.projectId);
  } else {
    projectReady = execute("firebase", ["use", project.firebase.projectId], { quiet: true });
    if (!projectReady) {
      console.log("[Firebase] Authentication may be expired; opening reauthentication...");
      execute("firebase", ["login", "--reauth"]);
      execute("firebase", ["login:use", project.firebase.account], { quiet: true });
      projectReady = execute("firebase", ["use", project.firebase.projectId], { quiet: true });
    }
  }
  if (!projectReady) {
    recovery("Firebase project selection failed", [
      `firebase login:use ${project.firebase.account}`,
      `firebase use ${project.firebase.projectId}`,
      "firebase projects:list",
    ]);
    return;
  }
  console.log(`[Firebase] Ready as ${project.firebase.account} on ${project.firebase.projectId}.`);
}

function configureGcloud() {
  console.log("\n[gcloud] Activating account, configuration, and ADC...");
  if (!requireCommand("gcloud", "winget install --id Google.CloudSDK")) return;

  const { configuration, account, projectId } = project.gcloud;

  let configurationExists = execute(
    "gcloud",
    ["config", "configurations", "describe", configuration],
    { quiet: true },
  );
  if (!configurationExists && !checkOnly) {
    configurationExists = execute(
      "gcloud",
      ["config", "configurations", "create", configuration, "--no-activate"],
      { quiet: true },
    );
  }

  let ready = configurationExists;
  if (ready && checkOnly) {
    const activeConfiguration = capture(
      "gcloud",
      ["config", "configurations", "list", "--filter=is_active:true", "--format=value(name)"],
    );
    const activeAccount = capture("gcloud", ["config", "get-value", "account"]);
    ready = activeConfiguration.ok
      && activeConfiguration.output === configuration
      && activeAccount.ok
      && activeAccount.output === account;
    if (projectId) {
      const activeProject = capture("gcloud", ["config", "get-value", "project"]);
      ready = ready && activeProject.ok && activeProject.output === projectId;
    }
  }
  if (ready && !checkOnly) {
    ready = execute("gcloud", ["config", "configurations", "activate", configuration], { quiet: true });
    ready = execute("gcloud", ["auth", "login", account, "--update-adc"]) && ready;
    ready = execute("gcloud", ["config", "set", "account", account], { quiet: true }) && ready;
    if (projectId) {
      ready = execute("gcloud", ["config", "set", "project", projectId], { quiet: true }) && ready;
      execute(
        "gcloud",
        ["auth", "application-default", "set-quota-project", projectId],
        { quiet: true },
      );
    }
  }

  const userTokenWorks = ready && execute(
    "gcloud",
    ["auth", "print-access-token", "--account", account],
    { quiet: true },
  );
  const adcWorks = ready && execute(
    "gcloud",
    ["auth", "application-default", "print-access-token"],
    { quiet: true },
  );

  if (!userTokenWorks || !adcWorks) {
    const commands = [
      `gcloud config configurations create ${configuration} --no-activate`,
      `gcloud config configurations activate ${configuration}`,
      `gcloud auth login ${account} --update-adc --force`,
      `gcloud config set account ${account}`,
    ];
    if (projectId) {
      commands.push(
        `gcloud config set project ${projectId}`,
        `gcloud auth application-default set-quota-project ${projectId}`,
      );
    }
    recovery("gcloud user credentials or Application Default Credentials failed", commands);
    return;
  }
  console.log(`[gcloud] Ready as ${account}${projectId ? ` on ${projectId}` : " (no project configured yet)"}.`);
}

console.log(`Starting ${project.label} credential session${checkOnly ? " (check only)" : ""}...`);
configureGit();
configureFirebase();
configureGcloud();

for (const note of notes) console.log(`\nNote: ${note}`);

if (failures.length > 0) {
  console.error("\nSession setup is incomplete. Run the commands below, then rerun npm start:");
  for (const failure of failures) {
    console.error(`\n${failure.area}:`);
    for (const command of failure.commands) console.error(`  ${command}`);
  }
  process.exitCode = 1;
} else {
  console.log(`\n${project.label} credentials are active. No commit or push was performed.`);
}
