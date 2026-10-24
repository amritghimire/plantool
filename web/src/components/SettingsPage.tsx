import { useEffect, useState } from "react";
import { NavLink, useLocation } from "react-router-dom";
import { api } from "../api";
import type { RepoInfo } from "../types";
import { SettingsDialog } from "./SettingsDialog";
import { GithubSettings } from "./GithubSettings";
import { ModelSettings } from "./ModelSettings";
import { WorkerSettings } from "./WorkerSettings";
import { OversightSettings } from "./OversightSettings";
import { LoopSettings } from "./LoopSettings";
import { NeverSentSettings } from "./NeverSentSettings";
import { RetentionSettings } from "./RetentionSettings";
const sections = [
  ["general", "Personal"],
  ["github", "GitHub & approvers"],
  ["worker", "Worker"],
  ["models", "Models & providers"],
  ["projects", "Project policies"],
  ["privacy", "Privacy & retention"],
];
export function SettingsPage() {
  const section = useLocation().pathname.split("/")[2] ?? "general";
  const [repos, setRepos] = useState<RepoInfo[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    if (section === "privacy") {
      let active = true;
      void Promise.allSettled([api.repos(), api.engineering()]).then(
        ([personal, engineering]) => {
          if (!active) return;
          const values =
            personal.status === "fulfilled" ? personal.value.repos : [];
          const projects =
            engineering.status === "fulfilled"
              ? engineering.value.projects.map((p) => ({
                  root: p.checkout,
                  repo_slug: p.name ?? p.id,
                  branch: p.branch ?? "main",
                  base: p.branch ?? "main",
                  sessions: 0,
                  last_used: p.synced_at ?? "",
                }))
              : [];
          setRepos([
            ...new Map(
              [...values, ...projects].map((r) => [r.root, r]),
            ).values(),
          ]);
          if (
            personal.status === "rejected" &&
            engineering.status === "rejected"
          )
            setError("Could not load repository privacy settings.");
        },
      );
      return () => {
        active = false;
      };
    }
  }, [section]);
  return (
    <main className="settings-page">
      <aside className="page-sidebar">
        <h1>Settings</h1>
        <p className="muted">
          Configure the application. Work happens in Engineering OS and
          Workspace.
        </p>
        <nav aria-label="Settings sections">
          {sections.map(([id, name]) => (
            <NavLink key={id} to={`/settings/${id}`}>
              {name}
            </NavLink>
          ))}
        </nav>
      </aside>
      <section className="settings-content" key={section}>
        {section === "general" ? (
          <SettingsDialog embedded onClose={() => {}} />
        ) : section === "github" ? (
          <>
            <h2>GitHub connections and approvers</h2>
            <p>
              App credentials and approvers are shared. Add repositories as
              separate projects in{" "}
              <a href="/engineering?add=1">Engineering OS</a>.
            </p>
            <GithubSettings mode="settings" />
          </>
        ) : section === "worker" ? (
          <>
            <h2>Worker service</h2>
            <p>
              The worker is shared by Engineering OS projects. See current work
              and graph history in <a href="/engineering">Engineering OS</a>.
            </p>
            <WorkerSettings showItems={false} />
          </>
        ) : section === "models" ? (
          <>
            <h2>Models and provider access</h2>
            <ModelSettings />
          </>
        ) : section === "projects" ? (
          <>
            <h2>Project policies and schedules</h2>
            <p>
              Each policy editor identifies its project. Changes apply to that
              project; Stop All is global.
            </p>
            <OversightSettings />
            <LoopSettings />
          </>
        ) : section === "privacy" ? (
          <>
            <h2>Privacy and retention</h2>
            <NeverSentSettings repos={repos} />
            <RetentionSettings />
            {error && <p role="alert">{error}</p>}
          </>
        ) : (
          <h2>Select a settings section</h2>
        )}
      </section>
    </main>
  );
}
