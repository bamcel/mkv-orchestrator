import { beforeEach, describe, expect, it } from "vitest";
import { applyWebTheme, getWebTheme, webThemes } from "./theme";

const chatStyleThemeNames = [
  "Absolutely",
  "Cappuccin",
  "Codex",
  "Everforest",
  "GitHub",
  "Gruvbox",
  "Linear",
  "Notion",
  "One",
  "Proof",
  "Raycast",
  "Rose Pine",
  "Solarized",
  "Vercel",
  "VS Code Plus",
  "Xcode"
];

describe("built-in themes", () => {
  beforeEach(() => window.localStorage.clear());

  it("includes every chat-style palette as a built-in theme", () => {
    expect(webThemes.map((theme) => theme.name)).toEqual(
      expect.arrayContaining(chatStyleThemeNames)
    );
  });

  it("includes media-server color schemes", () => {
    expect(webThemes.map((theme) => theme.name)).toEqual(
      expect.arrayContaining(["Emby", "Plex", "Jellyfin"])
    );
  });

  it("alphabetizes built-ins and migrates the former theme names", () => {
    const names = webThemes.map((theme) => theme.name);
    expect(names).toEqual([...names].sort((left, right) => left.localeCompare(right)));
    expect(names).toContain("Gotham");
    expect(names).toContain("Mercy");
    expect(names).not.toContain("Dark");
    expect(names).not.toContain("Light");
    expect(getWebTheme("Dark").name).toBe("Gotham");
    expect(getWebTheme("Light").name).toBe("Mercy");
  });

  it("applies the selected palette to the app immediately", () => {
    const theme = getWebTheme("Everforest");
    applyWebTheme(theme.name);

    expect(document.documentElement.style.getPropertyValue("--color-accent")).toBe(
      theme.colors.Accent
    );
    expect(window.localStorage.getItem("mkvo.web.theme")).toBe("Everforest");
  });
});
