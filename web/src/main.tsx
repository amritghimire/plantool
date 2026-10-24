import {GithubPlan} from "./components/GithubPlan";
import React from "react";
import { CleanBreakNotice } from "./components/CleanBreakNotice";
import {SetupWizard,StartPage} from "./components/SetupWizard";
import {AppShell} from "./components/AppShell";
import {SettingsPage} from "./components/SettingsPage";
import {EngineeringPage,EngineeringItemPage} from "./components/EngineeringPage";
import {WorkspacePage} from "./components/WorkspacePage";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { SessionList } from "./components/SessionList";
import { SessionPage } from "./components/SessionPage";
import "./styles.css";
import { AuthGate } from "./components/AuthGate";

function App() {
  return (
    <AuthGate><CleanBreakNotice /><BrowserRouter>
      <Routes><Route element={<AppShell/>}>
        <Route path="/engineering" element={<EngineeringPage/>}/>
        <Route path="/engineering/items/:id" element={<EngineeringItemPage/>}/>
        <Route path="/workspace" element={<WorkspacePage/>}/>
        <Route path="/settings/*" element={<SettingsPage/>}/>
        <Route path="/github/plans/:id" element={<GithubPlan/>} />
        <Route path="/" element={<StartPage />} />
        <Route path="/personal" element={<SessionList/>}/>
        <Route path="/setup/engineering" element={<SetupWizard/>}/>
        <Route path="/s/:repo/:slug" element={<SessionPage />} />
        <Route path="*" element={<SessionList />} />
      </Route></Routes>
    </BrowserRouter></AuthGate>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
