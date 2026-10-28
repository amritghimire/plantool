import { NavLink, Outlet } from "react-router-dom";
import { AccountMenu } from "./AuthGate";
import { ThemeToggle } from "./ThemeToggle";
import { ExecutionControls } from "./ExecutionControls";
export function AppShell() {
  return (
    <div className="app-shell">
      <a href="#app-content" className="skip-link">
        Skip to content
      </a>
      <header className="app-header">
        <NavLink to="/" className="brand">
          plantool
        </NavLink>
        <nav aria-label="Application">
          <NavLink to="/personal">Personal</NavLink>
          <NavLink to="/engineering">Engineering OS</NavLink>
          <NavLink to="/workspace">Workspace</NavLink>
          <NavLink to="/settings/access">Access &amp; permissions</NavLink>
        </nav>
        <div className="spacer" />
        <ExecutionControls />
        <ThemeToggle />
        <AccountMenu />
      </header>
      <div id="app-content" className="app-content">
        <Outlet />
      </div>
    </div>
  );
}
