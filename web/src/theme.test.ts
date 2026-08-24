import { beforeEach, describe, expect, it } from "vitest";
import { applyWebTheme, getWebTheme, loadCustomWebThemes, webThemes } from "./theme";

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
    expect(document.documentElement.style.getPropertyValue("--color-template")).toBe(
      theme.colors.TemplateHighlight
    );
    expect(window.localStorage.getItem("mkvo.web.theme")).toBe("Everforest");
  });

  it("gives every built-in theme its own configurable template highlight", () => {
    for (const theme of webThemes) {
      expect(theme.colors.TemplateHighlight).toBe(theme.colors.Accent);
    }
  });

  it("alphabetizes every theme color and includes the app-title color", () => {
    for (const theme of webThemes) {
      const colorNames = Object.keys(theme.colors);
      expect(colorNames).toEqual([...colorNames].sort((left, right) => left.localeCompare(right)));
      expect(theme.colors.AppTitle).toBeTruthy();
    }
  });

  it("migrates custom themes to use their accent for missing highlight colors", () => {
    window.localStorage.setItem("mkvo.web.customThemes", JSON.stringify([{
      name: "Legacy Custom",
      colors: { Accent: "#123456" }
    }]));

    const colors = loadCustomWebThemes()[0].colors;
    expect(colors.AppTitle).toBe("#123456");
    expect(colors.TemplateHighlight).toBe("#123456");
    expect(Object.keys(colors)).toEqual(["Accent", "AppTitle", "TemplateHighlight"]);
  });
});
