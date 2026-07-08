export type WebTheme = {
  name: string;
  colors: Record<string, string>;
};

const themeStorageKey = "mkvo.web.theme";
const customThemeStorageKey = "mkvo.web.customThemes";
const defaultThemeName = "Dark";
const legacyModernThemeName = "Modern";

export const webThemes: WebTheme[] = [
  {
    name: "Dark",
    colors: {
      Window: "#15171C",
      Card: "#20232A",
      Panel: "#252932",
      Sidebar: "#1B1E25",
      Input: "#1D2028",
      InputHover: "#292E38",
      Button: "#3B4252",
      ButtonHover: "#2E3440",
      Selected: "#2E3440",
      Border: "#3B4252",
      BorderStrong: "#4C566A",
      Text: "#ECEFF4",
      Muted: "#D8DEE9",
      Subtle: "#A7B0C0",
      Accent: "#BD93F9",
      AccentHover: "#2E3440",
      AppTitle: "#BD93F9",
      Success: "#50FA7B",
      Warning: "#EBCB8B",
      Disabled: "#7D8797",
      Brand: "#BD93F9"
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
      ButtonHover: "#3A3D4F",
      Selected: "#3A3D4F",
      Border: "#343746",
      BorderStrong: "#44475A",
      Text: "#F8F8F2",
      Muted: "#CFCFEA",
      Subtle: "#8B93A7",
      Accent: "#BD93F9",
      AccentHover: "#3A3D4F",
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
      Input: "#E8ECF4",
      InputHover: "#F1F4FA",
      Button: "#DCE3EF",
      ButtonHover: "#6D5BD0",
      Selected: "#D9DDF0",
      Border: "#CAD2E0",
      BorderStrong: "#9DA8BA",
      Text: "#1C2430",
      Muted: "#46556A",
      Subtle: "#66758A",
      Accent: "#6D5BD0",
      AccentHover: "#6D5BD0",
      AppTitle: "#6D5BD0",
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
  defaultNames.add(legacyModernThemeName.toLowerCase());
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
  defaultNames.add(legacyModernThemeName.toLowerCase());
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
    window.localStorage.setItem(themeStorageKey, defaultThemeName);
  }

  return getAllWebThemes();
}

export function getStoredWebThemeName(): string {
  try {
    const saved = window.localStorage.getItem(themeStorageKey);
    const normalized = normalizeThemeName(saved);
    return getAllWebThemes().some((theme) => theme.name === normalized) ? normalized : defaultThemeName;
  } catch {
    return defaultThemeName;
  }
}

export function getWebTheme(name: string | null | undefined): WebTheme {
  const normalized = normalizeThemeName(name);
  return getAllWebThemes().find((theme) => theme.name === normalized) ?? webThemes[0];
}

function normalizeThemeName(name: string | null | undefined) {
  if (!name) return defaultThemeName;
  return name === legacyModernThemeName ? defaultThemeName : name;
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
