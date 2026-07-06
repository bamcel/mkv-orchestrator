export type WebTheme = {
  name: string;
  colors: Record<string, string>;
};

const themeStorageKey = "mkvo.web.theme";
const customThemeStorageKey = "mkvo.web.customThemes";

export const webThemes: WebTheme[] = [
  {
    name: "Modern",
    colors: {
      Window: "#090B0F",
      Card: "#15181E",
      Panel: "#1D222A",
      Sidebar: "#111318",
      Input: "#090B0F",
      InputHover: "#20252D",
      Button: "#20242C",
      ButtonHover: "#2B313C",
      Selected: "#252A33",
      Border: "#2B313C",
      BorderStrong: "#3A4350",
      Text: "#F5F7FB",
      Muted: "#A9B6CC",
      Subtle: "#728198",
      Accent: "#24D184",
      AccentHover: "#39E695",
      AppTitle: "#F5F7FB",
      Success: "#24D184",
      Warning: "#F4A261",
      Disabled: "#5E6878",
      Brand: "#8B5CF6"
    }
  },
  {
    name: "Midnight",
    colors: {
      Window: "#1E1F29",
      Card: "#282A36",
      Panel: "#2B2E3A",
      Sidebar: "#232631",
      Input: "#282A36",
      InputHover: "#2F3140",
      Button: "#44475A",
      ButtonHover: "#BD93F9",
      Selected: "#3A3D4F",
      Border: "#343746",
      BorderStrong: "#44475A",
      Text: "#F8F8F2",
      Muted: "#CFCFEA",
      Subtle: "#8B93A7",
      Accent: "#BD93F9",
      AccentHover: "#C9A7FA",
      AppTitle: "#BD93F9",
      Success: "#50FA7B",
      Warning: "#FFA500",
      Disabled: "#6272A4",
      Brand: "#BD93F9"
    }
  },
  {
    name: "Light",
    colors: {
      Window: "#F5F6FA",
      Card: "#E8ECF4",
      Panel: "#EEF1F7",
      Sidebar: "#E8ECF4",
      Input: "#F8FAFD",
      InputHover: "#F1F4FA",
      Button: "#DCE3EF",
      ButtonHover: "#C8D1E1",
      Selected: "#D9DDF0",
      Border: "#CAD2E0",
      BorderStrong: "#9DA8BA",
      Text: "#1C2430",
      Muted: "#46556A",
      Subtle: "#66758A",
      Accent: "#24A46D",
      AccentHover: "#1E8D5E",
      AppTitle: "#1C2430",
      Success: "#17803D",
      Warning: "#A15C00",
      Disabled: "#8792A3",
      Brand: "#6D5BD0"
    }
  }
];

const cssVariableMap: Record<string, string> = {
  Window: "--color-window",
  Card: "--color-card",
  Panel: "--color-panel",
  Sidebar: "--color-sidebar",
  Input: "--color-input",
  InputHover: "--color-input-hover",
  Button: "--color-button",
  ButtonHover: "--color-button-hover",
  Selected: "--color-selected",
  Border: "--color-border",
  BorderStrong: "--color-border-strong",
  Text: "--color-text",
  Muted: "--color-muted",
  Subtle: "--color-subtle",
  Accent: "--color-accent",
  AccentHover: "--color-accent-hover",
  AppTitle: "--color-app-title",
  Success: "--color-success",
  Warning: "--color-warning",
  Disabled: "--color-disabled",
  Brand: "--color-brand"
};

export function loadCustomWebThemes(): WebTheme[] {
  try {
    const raw = window.localStorage.getItem(customThemeStorageKey);
    if (!raw) return [];

    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];

    return parsed
      .filter((theme): theme is WebTheme =>
        theme
        && typeof theme.name === "string"
        && theme.name.trim().length > 0
        && theme.colors
        && typeof theme.colors === "object")
      .map((theme) => ({ name: theme.name.trim(), colors: theme.colors }));
  } catch {
    return [];
  }
}

export function getAllWebThemes(): WebTheme[] {
  const defaultNames = new Set(webThemes.map((theme) => theme.name.toLowerCase()));
  const customThemes = loadCustomWebThemes().filter((theme) => !defaultNames.has(theme.name.toLowerCase()));
  return [...webThemes, ...customThemes];
}

export function saveCustomWebTheme(theme: WebTheme) {
  const cleanTheme = {
    name: theme.name.trim(),
    colors: theme.colors
  };
  if (!cleanTheme.name) return getAllWebThemes();

  const defaultNames = new Set(webThemes.map((item) => item.name.toLowerCase()));
  if (defaultNames.has(cleanTheme.name.toLowerCase())) return getAllWebThemes();

  const nextThemes = [
    ...loadCustomWebThemes().filter((item) => item.name.toLowerCase() !== cleanTheme.name.toLowerCase()),
    cleanTheme
  ].sort((left, right) => left.name.localeCompare(right.name));

  window.localStorage.setItem(customThemeStorageKey, JSON.stringify(nextThemes));
  return getAllWebThemes();
}

export function removeCustomWebTheme(name: string) {
  const nextThemes = loadCustomWebThemes().filter((theme) => theme.name.toLowerCase() !== name.toLowerCase());
  window.localStorage.setItem(customThemeStorageKey, JSON.stringify(nextThemes));
  if (getStoredWebThemeName().toLowerCase() === name.toLowerCase()) {
    window.localStorage.setItem(themeStorageKey, "Modern");
  }

  return getAllWebThemes();
}

export function getStoredWebThemeName(): string {
  try {
    const saved = window.localStorage.getItem(themeStorageKey);
    return getAllWebThemes().some((theme) => theme.name === saved) ? saved! : "Modern";
  } catch {
    return "Modern";
  }
}

export function getWebTheme(name: string | null | undefined): WebTheme {
  return getAllWebThemes().find((theme) => theme.name === name) ?? webThemes[0];
}

export function applyWebTheme(name: string) {
  const theme = getWebTheme(name);
  const root = document.documentElement;

  for (const [colorName, variableName] of Object.entries(cssVariableMap)) {
    const color = theme.colors[colorName];
    if (color) root.style.setProperty(variableName, color);
  }

  root.style.backgroundColor = theme.colors.Window;
  document.body.style.backgroundColor = theme.colors.Window;
  document.body.style.color = theme.colors.Text;

  try {
    window.localStorage.setItem(themeStorageKey, theme.name);
  } catch {
    // Theme persistence is optional; applying the current session theme is enough.
  }

  return theme;
}
