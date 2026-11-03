import { describe, expect, it } from "vitest";
import { fileUrl, sessionFilePath } from "./files";

describe("session files", () => {
  it("maps relative document links to session files", () => {
    expect(sessionFilePath("./screens/login%20page.png")).toBe("screens/login page.png");
    expect(sessionFilePath("notes.md#top")).toBe("notes.md");
    expect(sessionFilePath("../screens/a.png", "notes")).toBe("screens/a.png");
    expect(sessionFilePath("../../secret.md", "notes")).toBeNull();
    expect(fileUrl("repo/fix", "screens/login page.png")).toBe("/api/sessions/repo/fix/files/screens/login%20page.png");
  });

  it("leaves external, absolute, anchor and parent links alone", () => {
    for (const href of ["https://x.dev/a.png", "data:image/png;base64,x", "/etc/passwd", "#heading", "../secret.md", "mailto:a@b.c", undefined]) {
      expect(sessionFilePath(href)).toBeNull();
    }
  });
});
