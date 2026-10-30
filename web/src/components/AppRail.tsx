import { Link, useLocation } from "react-router-dom";
import {
  SquaresFour,
  Plus,
  GearSix,
  BookOpen,
  Stack,
  Desktop,
} from "@phosphor-icons/react";
import { ThemeToggle } from "./ThemeToggle";
import { cn } from "../lib/cn";

export function AppRail({
  onNew,
  onSettings,
}: {
  onNew?: () => void;
  onSettings?: () => void;
}) {
  const { pathname } = useLocation();
  return (
    <nav className="app-rail" aria-label="Application">
      <Link to="/" className="rail-logo" aria-label="Plantool home">
        <Stack size={26} weight="bold" aria-hidden="true" />
      </Link>
      <div className="rail-group">
        <Link
          to="/"
          className={cn("rail-button", pathname === "/" && "selected")}
          aria-label="Sessions"
          title="Sessions"
        >
          <SquaresFour size={22} aria-hidden="true" />
        </Link>
        {onNew ? (
          <button
            className="rail-button"
            type="button"
            onClick={onNew}
            aria-label="Create session"
            title="Create session"
          >
            <Plus size={22} aria-hidden="true" />
          </button>
        ) : (
          <Link
            className="rail-button"
            to="/?new=1"
            aria-label="Create session"
            title="Create session"
          >
            <Plus size={22} aria-hidden="true" />
          </Link>
        )}
      </div>
      <div className="rail-bottom">
        <a
          className="rail-button"
          href="https://github.com/amritghimire/plantool#readme"
          target="_blank"
          rel="noreferrer"
          aria-label="Documentation"
          title="Documentation"
        >
          <BookOpen size={21} aria-hidden="true" />
        </a>
        {onSettings ? (
          <button
            className="rail-button"
            type="button"
            onClick={onSettings}
            aria-label="Settings"
            title="Settings"
          >
            <GearSix size={21} aria-hidden="true" />
          </button>
        ) : (
          <Link
            className="rail-button"
            to="/?settings=1"
            aria-label="Settings"
            title="Settings"
          >
            <GearSix size={21} aria-hidden="true" />
          </Link>
        )}
        <ThemeToggle />
        <span className="rail-local" title="Runs on your computer">
          <Desktop size={20} aria-hidden="true" />
          <span className="sr-only">Local workspace</span>
        </span>
      </div>
    </nav>
  );
}
