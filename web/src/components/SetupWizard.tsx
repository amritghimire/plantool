import { useEffect, useState } from "react";
import { Link, Navigate, useNavigate } from "react-router-dom";
import { api } from "../api";
import { usePolling } from "../lib/usePolling";
import { GithubSettings } from "./GithubSettings";
import { WorkerSettings } from "./WorkerSettings";
import { ModelSettings } from "./ModelSettings";
import { OversightSettings } from "./OversightSettings";
import { LoopSettings } from "./LoopSettings";
const steps = [
  ["welcome", "Welcome"],
  ["tools", "Model tools"],
  ["models", "Model routes"],
  ["github", "Connect project"],
  ["policies", "Project policies"],
  ["worker", "Worker"],
  ["ready", "First run"],
];
export function SetupWizard() {
  const nav = useNavigate();
  const overview = usePolling(api.engineering);
  const [cursor, setCursor] = useState<{
    step: string;
    project: string | null;
  } | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  useEffect(() => {
    let active = true;
    api
      .setupProgress()
      .then((p) => {
        if (active)
          setCursor(
            steps.some(([id]) => id === p.step) ? p : { ...p, step: "welcome" },
          );
      })
      .catch((e) => {
        if (active) setError((e as Error).message);
      });
    return () => {
      active = false;
    };
  }, []);
  const index = steps.findIndex(([id]) => id === cursor?.step);
  const move = async (step: string, project = cursor?.project ?? null) => {
    setBusy(true);
    setError("");
    try {
      const saved = await api.saveSetupProgress({ step, project });
      setCursor(saved);
      return true;
    } catch (e) {
      setError((e as Error).message);
      return false;
    } finally {
      setBusy(false);
    }
  };
  const next = async () => {
    if (!cursor) return;
    setBusy(true);
    setError("");
    try {
      if (
        cursor.step === "github" &&
        !(await api.engineering()).projects.some((p) => p.id === cursor.project)
      )
        throw new Error("Connect or select a project before continuing.");
      if (cursor.step === "worker") {
        const status = await api.workerStatus();
        if (!status.worker.available) throw new Error(status.worker.message);
      }
      if (cursor.step === "models") {
        const execution = await api.executionStatus();
        if (!execution.execution.stopped)
          throw new Error(
            "Use Stop All in the header before configuring setup. It affects all projects and Personal runs.",
          );
        await api.verifyModels();
        const readiness = (await api.engineering()).roles ?? [];
        const missing = readiness.filter((r) => !r.ready).map((r) => r.role);
        if (readiness.length !== 9 || missing.length)
          throw new Error(
            `No verified route for: ${missing.join(", ") || "all roles"}. Configure a usable route for every role before continuing.`,
          );
        setNotice(
          "Every role has a catalog-verified route. A real run is still needed to verify provider execution.",
        );
      }
      await move(steps[index + 1]?.[0] ?? "ready");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const finish = async () => {
    if (!cursor?.project) return;
    setBusy(true);
    setError("");
    try {
      const worker = await api.workerStatus();
      if (!worker.worker.available) throw new Error(worker.worker.message);
      const project = (await api.engineering()).projects.find(
        (p) => p.id === cursor.project,
      );
      if (!project)
        throw new Error(
          "Select a connected project before opening Engineering OS.",
        );
      const missing = (project.roles ?? [])
        .filter((r) => !r.ready)
        .map((r) => r.role);
      if (project.roles?.length !== 9 || missing.length)
        throw new Error(
          `Review model routes for this project's allowed providers: ${missing.join(", ") || "all roles"}.`,
        );
      if (await move("ready"))
        nav(`/engineering?project=${encodeURIComponent(cursor.project)}`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <main className="setup-page">
      <aside className="page-sidebar">
        <div className="eyebrow">ENGINEERING OS</div>
        <h1>Guided setup</h1>
        <p>
          Your step and project are saved on this daemon. Come back after
          closing the browser or restarting. Unsaved form inputs are not stored.
        </p>
        <ol className="setup-steps">
          {steps.map(([id, name], i) => (
            <li key={id}>
              <button
                disabled={busy || !cursor || i > index}
                aria-current={cursor?.step === id ? "step" : undefined}
                onClick={() => void move(id)}
              >
                <span>{i + 1}</span>
                {name}
              </button>
            </li>
          ))}
        </ol>
        <Link to="/engineering">Save for later · open Engineering OS</Link>
      </aside>
      <section className="setup-content">
        <header>
          <div className="eyebrow">
            STEP {Math.max(index + 1, 1)} OF {steps.length}
          </div>
          <h2>{steps[index]?.[1] ?? "Resume setup"}</h2>
        </header>
        {error && (
          <p role="alert" className="error">
            {error}
          </p>
        )}
        {notice && <p role="status">{notice}</p>}
        {!cursor ? (
          <p>Loading your saved place…</p>
        ) : (
          <>
            {cursor.step === "welcome" && (
              <>
                <h3>Your engineering team, across projects</h3>
                <p>
                  Set up the shared worker and model connections, then connect
                  one or more repositories. Each project has its own checkout,
                  policies and schedule.
                </p>
                <p>
                  Personal remains available independently. Engineering OS needs
                  macOS, a platform passkey and sandbox-exec. This browser is
                  already signed in; security changes below will ask for a fresh
                  passkey.
                </p>
                <div className="onboarding-card">
                  <strong>What you will need</strong>
                  <ul>
                    <li>
                      A local checkout and access to its GitHub repository.
                    </li>
                    <li>A GitHub App installed on that repository.</li>
                    <li>
                      Signed-in model tools and any provider keys you choose.
                    </li>
                  </ul>
                </div>
              </>
            )}
            {cursor.step === "tools" && (
              <>
                <h3>Install and sign in on the daemon's Mac</h3>
                <p>
                  If these tools are already installed, reuse them. PlanTool's
                  binary needs no Rust or Nix. Node is for npm-installed model
                  CLIs.
                </p>
                <pre className="setup-command">
                  {
                    'brew install git gh node@26\nexport PATH="$(brew --prefix node@26)/bin:$PATH"\nnpm install -g @openai/codex opencode-ai\nbrew install --cask ollama\nopen -a Ollama\ncodex login\nollama signin\nollama list'
                  }
                </pre>
                <p>
                  Ollama cloud uses the local signed-in service; no Ollama API
                  key is needed. Pull actual model names from its current
                  catalog. Codex login supplies only Codex, not other providers.
                </p>
                <p>
                  For OpenRouter or Zen, open Keychain Access, choose Continue
                  if macOS offers the Passwords app, select login → File → New
                  Password Item. Use service <code>plantool/openrouter</code> or{" "}
                  <code>plantool/opencode-zen</code>, your account name and the
                  provider API key as the password. Do not put keys in YAML.
                </p>
                <p>
                  Make CLI paths persistent in your shell, then restart the idle
                  PlanTool daemon from that terminal if you changed PATH. You
                  can return to this saved step afterward.
                </p>
              </>
            )}
            {cursor.step === "github" && (
              <>
                <p>
                  Select a connected project to continue, or add another
                  repository. App credentials are shared; the checkout belongs
                  to this project. Start with a disposable playground:
                  connecting enables issue intake.
                </p>
                <label>
                  Setup project
                  <select
                    value={cursor.project ?? ""}
                    disabled={busy}
                    onChange={(e) =>
                      void move("github", e.target.value || null)
                    }
                  >
                    <option value="">Choose a project</option>
                    {overview.data?.projects.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name ?? p.id}
                      </option>
                    ))}
                  </select>
                </label>
                <GithubSettings
                  mode="projects"
                  onConfigured={(project) => {
                    overview.refresh();
                    if (project) void move("github", project);
                  }}
                />
              </>
            )}
            {cursor.step === "worker" && (
              <>
                <p>
                  Resume execution with passkey in the header first. Then Repair
                  installs Python 3.12 and the locked worker automatically under
                  PlanTool's data directory. No global Python installation is
                  necessary. It stays sandboxed and authenticates to the daemon
                  over a Unix socket.
                </p>
                <WorkerSettings expanded showItems={false} />
                <p>
                  If repair fails, keep the error and your place here. Repeated
                  repair does not fix an unexplained startup failure.
                </p>
              </>
            )}
            {cursor.step === "models" && (
              <>
                <p>
                  Use Stop All in the header before changing policy. Verify
                  routes, check availability for every role including Docs and
                  Supervisor, and keep execution stopped until the Worker step.
                </p>
                <p>
                  Ollama and Codex sign-ins do not make every default chain
                  usable. Enable only providers you intend to use; configure
                  explicit fallback choices rather than assuming unavailable IDs
                  will be substituted.
                </p>
                <ModelSettings expanded />
              </>
            )}
            {cursor.step === "policies" && (
              <>
                <p>
                  Choose your project in each editor. Start with Minimal for the
                  first controlled run. Confirm timezone, sprint time, earlier
                  scan time and provider privacy settings. Leave Nix off unless
                  you want it and have it installed.
                </p>
                <p>
                  These settings affect the selected project. Stop All affects
                  every project and Personal runs. Resuming does not
                  automatically restart cancelled graphs.
                </p>
                <OversightSettings expanded />
                <LoopSettings expanded />
                <p>
                  Review{" "}
                  <Link to="/settings/privacy">
                    never-sent files and retention
                  </Link>
                  ; your setup place stays saved.
                </p>
              </>
            )}
            {cursor.step === "ready" && (
              <>
                <h3>Try one small issue first</h3>
                <p>
                  Create a small, testable issue as an allowlisted owner. PM
                  triages it. To prepare its plan immediately, post{" "}
                  <code>/plantool urgent</code> on that issue; otherwise wait
                  for the project schedule.
                </p>
                <ol>
                  <li>
                    Read the App's plan and approve its current hash using the
                    exact command it provides.
                  </li>
                  <li>
                    Watch role attempts and recorded graph steps in Engineering
                    OS.
                  </li>
                  <li>
                    Read team discussion in Workspace; owner messages are
                    context only.
                  </li>
                  <li>
                    Review the final PR, wait for current-head CI and submit an
                    Approve review on GitHub.
                  </li>
                </ol>
                <p>
                  Setup navigation is not an approval. Engineering OS will still
                  enforce all plan, milestone and PR gates.
                </p>
              </>
            )}
            <footer className="setup-actions">
              <button
                className="btn"
                disabled={busy || index === 0}
                onClick={() => void move(steps[Math.max(index - 1, 0)][0])}
              >
                Back
              </button>
              <span className="small muted">
                Progress saved automatically · {steps[index]?.[1]}
              </span>
              {cursor.step === "ready" ? (
                <button
                  className="btn primary"
                  disabled={busy || !cursor.project}
                  onClick={() => void finish()}
                >
                  Open my project
                </button>
              ) : (
                <button
                  className="btn primary"
                  disabled={busy}
                  onClick={() => void next()}
                >
                  {busy
                    ? "Saving…"
                    : cursor.step === "models"
                      ? "Verify and continue"
                      : "Continue"}
                </button>
              )}
            </footer>
          </>
        )}
      </section>
    </main>
  );
}
export function StartPage() {
  const [progress, setProgress] = useState<
    { step: string; project: string | null } | null | undefined
  >(undefined);
  useEffect(() => {
    let active = true;
    api
      .setupProgress()
      .then((p) => {
        if (active) setProgress(p);
      })
      .catch(() => {
        if (active) setProgress(null);
      });
    return () => {
      active = false;
    };
  }, []);
  if (progress === undefined) return <p role="status">Opening PlanTool…</p>;
  return (
    <Navigate
      replace
      to={
        progress?.step === "ready" && progress.project
          ? `/engineering?project=${encodeURIComponent(progress.project)}`
          : "/personal"
      }
    />
  );
}
