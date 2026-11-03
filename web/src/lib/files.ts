export interface SessionFile {
  path: string;
  size: number;
  kind: "image" | "markdown" | "text" | "other";
}

export function fileUrl(key: string, path: string): string {
  return `/api/sessions/${key}/files/${path.split("/").map(encodeURIComponent).join("/")}`;
}

// A link or image in a session document (in folder `base`) that points at another file in the session directory.
export function sessionFilePath(href: string | undefined, base = ""): string | null {
  if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("/") || href.startsWith("#")) return null;
  const parts: string[] = [];
  for (const part of [...base.split("/"), ...decodeURI(href.split(/[?#]/)[0]).split("/")]) {
    if (part === "..") {
      if (!parts.pop()) return null;
    } else if (part && part !== ".") parts.push(part);
  }
  return parts.length ? parts.join("/") : null;
}
