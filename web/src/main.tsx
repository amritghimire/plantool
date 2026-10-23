import React from "react";
import { CleanBreakNotice } from "./components/CleanBreakNotice";
import { ExecutionControls } from "./components/ExecutionControls";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { SessionList } from "./components/SessionList";
import { SessionPage } from "./components/SessionPage";
import "./styles.css";
import { AuthGate } from "./components/AuthGate";

function App() {
  return (
    <AuthGate><CleanBreakNotice /><ExecutionControls /><BrowserRouter>
      <Routes>
        <Route path="/" element={<SessionList />} />
        <Route path="/s/:repo/:slug" element={<SessionPage />} />
        <Route path="*" element={<SessionList />} />
      </Routes>
    </BrowserRouter></AuthGate>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
