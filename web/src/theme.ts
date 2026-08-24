export type WebTheme = {
  name: string;
  colors: Record<string, string>;
};

const themeStorageKey = "mkvo.web.theme";
const customThemeStorageKey = "mkvo.web.customThemes";
const defaultThemeName = "Gotham";
const legacyBuiltInThemeNames = ["Modern", "Dark", "Light"];

type Palette = {
  window: string;
  card: string;
  panel: string;
  sidebar: string;
  input: string;
  button: string;
  hover: string;
  selected: string;
  border: string;
  borderStrong: string;
  text: string;
  muted: string;
  subtle: string;
  accent: string;
  accentHover: string;
  success: string;
  warning: string;
  disabled: string;
};

function paletteTheme(name: string, palette: Palette): WebTheme {
  return {
    name,
    colors: {
      Window: palette.window,
      Card: palette.card,
      Panel: palette.panel,
      Sidebar: palette.sidebar,
      Input: palette.input,
      InputHover: palette.hover,
      Button: palette.button,
      ButtonHover: palette.hover,
      Selected: palette.selected,
      Border: palette.border,
      BorderStrong: palette.borderStrong,
      Text: palette.text,
      Muted: palette.muted,
      Subtle: palette.subtle,
      Accent: palette.accent,
      AccentHover: palette.accentHover,
      TemplateHighlight: palette.accent,
      AppTitle: palette.accent,
      Success: palette.success,
      Warning: palette.warning,
      Disabled: palette.disabled,
      Brand: palette.accent
    }
  };
}

export const webThemes: WebTheme[] = [
  {
    name: "Gotham",
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
      TemplateHighlight: "#BD93F9",
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
      TemplateHighlight: "#BD93F9",
      AppTitle: "#BD93F9",
      Success: "#50FA7B",
      Warning: "#FFA500",
      Disabled: "#6272A4",
      Brand: "#BD93F9"
    }
  },
  {
    name: "Mercy",
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
      TemplateHighlight: "#6D5BD0",
      AppTitle: "#6D5BD0",
      Success: "#17803D",
      Warning: "#A15C00",
      Disabled: "#8792A3",
      Brand: "#6D5BD0"
    }
  },
  paletteTheme("Absolutely", {
    window: "#F6F3EE", card: "#FFFCF8", panel: "#F1ECE5", sidebar: "#E9E3DB",
    input: "#F4EFE9", button: "#E8DED4", hover: "#DED2C7", selected: "#F0DDD0",
    border: "#D6CCC2", borderStrong: "#B9AA9D", text: "#292522", muted: "#655D56",
    subtle: "#887D74", accent: "#C66B43", accentHover: "#AD5835", success: "#347A4A",
    warning: "#9A6218", disabled: "#A79D95"
  }),
  paletteTheme("Cappuccin", {
    window: "#1E1E2E", card: "#252538", panel: "#2B2B40", sidebar: "#181825",
    input: "#313147", button: "#3B3B54", hover: "#484864", selected: "#403854",
    border: "#45455F", borderStrong: "#62627C", text: "#CDD6F4", muted: "#BAC2DE",
    subtle: "#9399B2", accent: "#CBA6F7", accentHover: "#B58BE8", success: "#A6E3A1",
    warning: "#F9E2AF", disabled: "#6C7086"
  }),
  paletteTheme("Codex", {
    window: "#181A1F", card: "#202329", panel: "#252930", sidebar: "#15171B",
    input: "#2A2E36", button: "#303640", hover: "#3A424E", selected: "#263C4A",
    border: "#3A414C", borderStrong: "#566170", text: "#F1F3F5", muted: "#C5CAD1",
    subtle: "#929AA5", accent: "#3B9EFF", accentHover: "#2188E8", success: "#42C77A",
    warning: "#E7B65A", disabled: "#69727E"
  }),
  paletteTheme("Everforest", {
    window: "#272E33", card: "#2E383C", panel: "#343F44", sidebar: "#232A2E",
    input: "#374145", button: "#414B50", hover: "#4B565C", selected: "#3C4F48",
    border: "#475258", borderStrong: "#68757A", text: "#D3C6AA", muted: "#B7B09A",
    subtle: "#859289", accent: "#A7C080", accentHover: "#91AD6C", success: "#83C092",
    warning: "#DBBC7F", disabled: "#6D7A72"
  }),
  paletteTheme("GitHub", {
    window: "#0D1117", card: "#161B22", panel: "#1C2128", sidebar: "#010409",
    input: "#0D1117", button: "#21262D", hover: "#30363D", selected: "#1F3042",
    border: "#30363D", borderStrong: "#484F58", text: "#E6EDF3", muted: "#B1BAC4",
    subtle: "#7D8590", accent: "#2F81F7", accentHover: "#1F6FEB", success: "#3FB950",
    warning: "#D29922", disabled: "#6E7681"
  }),
  paletteTheme("Gruvbox", {
    window: "#282828", card: "#32302F", panel: "#3C3836", sidebar: "#1D2021",
    input: "#3C3836", button: "#504945", hover: "#665C54", selected: "#4A4435",
    border: "#504945", borderStrong: "#7C6F64", text: "#EBDBB2", muted: "#D5C4A1",
    subtle: "#A89984", accent: "#D79921", accentHover: "#B57614", success: "#98971A",
    warning: "#FE8019", disabled: "#7C6F64"
  }),
  paletteTheme("Linear", {
    window: "#17171A", card: "#1F1F23", panel: "#25252A", sidebar: "#121214",
    input: "#29292F", button: "#303038", hover: "#3A3A44", selected: "#343047",
    border: "#35353D", borderStrong: "#51515E", text: "#F1F1F3", muted: "#C5C5CC",
    subtle: "#8B8B96", accent: "#7C6AEF", accentHover: "#6957DC", success: "#4AC58B",
    warning: "#E0AA55", disabled: "#676771"
  }),
  paletteTheme("Notion", {
    window: "#F7F7F5", card: "#FFFFFF", panel: "#F1F1EF", sidebar: "#F0F0EE",
    input: "#F7F7F5", button: "#E9E9E7", hover: "#DEDEDB", selected: "#E3ECF4",
    border: "#D9D9D6", borderStrong: "#B6B6B2", text: "#252525", muted: "#5F5E5A",
    subtle: "#85847F", accent: "#0B6E99", accentHover: "#095E83", success: "#448361",
    warning: "#A66A18", disabled: "#A09F9A"
  }),
  paletteTheme("One", {
    window: "#21252B", card: "#282C34", panel: "#2C313A", sidebar: "#1E2228",
    input: "#2F343E", button: "#3A404B", hover: "#464D59", selected: "#333F55",
    border: "#3E4451", borderStrong: "#5C6370", text: "#ABB2BF", muted: "#C8CDD5",
    subtle: "#7F8794", accent: "#61AFEF", accentHover: "#4D9BD8", success: "#98C379",
    warning: "#E5C07B", disabled: "#5C6370"
  }),
  paletteTheme("Proof", {
    window: "#F4F5F0", card: "#FCFDF9", panel: "#ECEFE8", sidebar: "#E8EBE5",
    input: "#F2F4EF", button: "#E1E6DF", hover: "#D5DDD7", selected: "#DDEAE3",
    border: "#CED6D0", borderStrong: "#AAB8AF", text: "#26332D", muted: "#52645A",
    subtle: "#77877E", accent: "#3D7660", accentHover: "#315F4D", success: "#3C805A",
    warning: "#95691F", disabled: "#98A39D"
  }),
  paletteTheme("Raycast", {
    window: "#171719", card: "#202024", panel: "#27272C", sidebar: "#121214",
    input: "#2B2B31", button: "#35353C", hover: "#414149", selected: "#493137",
    border: "#3D3D44", borderStrong: "#5B5B64", text: "#FAFAFA", muted: "#D0CFD3",
    subtle: "#929198", accent: "#FF5A67", accentHover: "#E94B58", success: "#55C994",
    warning: "#E4B45E", disabled: "#6D6C73"
  }),
  paletteTheme("Rose Pine", {
    window: "#191724", card: "#1F1D2E", panel: "#26233A", sidebar: "#14121E",
    input: "#26233A", button: "#312E45", hover: "#3B3752", selected: "#403044",
    border: "#393552", borderStrong: "#6E6A86", text: "#E0DEF4", muted: "#C4A7E7",
    subtle: "#908CAA", accent: "#EB6F92", accentHover: "#D85F82", success: "#9CCFD8",
    warning: "#F6C177", disabled: "#6E6A86"
  }),
  paletteTheme("Solarized", {
    window: "#FDF6E3", card: "#EEE8D5", panel: "#F7F0DC", sidebar: "#E8E2CF",
    input: "#F5EEDB", button: "#DED8C5", hover: "#D2CCB9", selected: "#DDE8E4",
    border: "#D0C9B5", borderStrong: "#93A1A1", text: "#073642", muted: "#586E75",
    subtle: "#839496", accent: "#B58900", accentHover: "#987400", success: "#2AA198",
    warning: "#CB4B16", disabled: "#A4AAA4"
  }),
  paletteTheme("Vercel", {
    window: "#0A0A0A", card: "#111111", panel: "#171717", sidebar: "#050505",
    input: "#1A1A1A", button: "#242424", hover: "#303030", selected: "#16263B",
    border: "#2E2E2E", borderStrong: "#505050", text: "#EDEDED", muted: "#B7B7B7",
    subtle: "#888888", accent: "#0070F3", accentHover: "#0060D1", success: "#46A758",
    warning: "#E5A000", disabled: "#666666"
  }),
  paletteTheme("VS Code Plus", {
    window: "#181818", card: "#1F1F1F", panel: "#252526", sidebar: "#181818",
    input: "#2A2D2E", button: "#333337", hover: "#3E3E42", selected: "#24394A",
    border: "#3C3C3C", borderStrong: "#5A5A5A", text: "#CCCCCC", muted: "#B8B8B8",
    subtle: "#858585", accent: "#007ACC", accentHover: "#006BB3", success: "#4EC9B0",
    warning: "#DCDCAA", disabled: "#666666"
  }),
  paletteTheme("Xcode", {
    window: "#F2F4F7", card: "#FFFFFF", panel: "#E9EDF2", sidebar: "#E5E9EF",
    input: "#F7F8FA", button: "#DFE4EA", hover: "#D2D8E0", selected: "#DCEBFA",
    border: "#CBD1D9", borderStrong: "#A3ABB6", text: "#1F2328", muted: "#505963",
    subtle: "#747E89", accent: "#006FE6", accentHover: "#005FC7", success: "#348A42",
    warning: "#A86400", disabled: "#99A1AA"
  })
].map(normalizeWebTheme).sort((left, right) => left.name.localeCompare(right.name));

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
  TemplateHighlight: "--color-template",
  AppTitle: "--color-app-title",
  Success: "--color-success",
  Warning: "--color-warning",
  Disabled: "--color-disabled",
  Brand: "--color-brand"
};

export function normalizeWebTheme(theme: WebTheme): WebTheme {
  const colors = {
    ...theme.colors,
    AppTitle: theme.colors.AppTitle || theme.colors.Brand || theme.colors.Accent || "#BD93F9",
    TemplateHighlight: theme.colors.TemplateHighlight || theme.colors.Accent || "#BD93F9"
  };

  return {
    name: theme.name.trim(),
    colors: Object.fromEntries(
      Object.entries(colors).sort(([left], [right]) => left.localeCompare(right))
    )
  };
}

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
      .map(normalizeWebTheme);
  } catch {
    return [];
  }
}

export function replaceCustomWebThemes(themes: WebTheme[]): WebTheme[] {
  const defaultNames = new Set(webThemes.map((theme) => theme.name.toLowerCase()));
  legacyBuiltInThemeNames.forEach((name) => defaultNames.add(name.toLowerCase()));
  const normalized = themes
    .filter((theme) => theme.name.trim() && theme.colors && typeof theme.colors === "object")
    .map(normalizeWebTheme)
    .filter((theme) => !defaultNames.has(theme.name.toLowerCase()))
    .filter((theme, index, all) =>
      all.findIndex((candidate) => candidate.name.toLowerCase() === theme.name.toLowerCase()) === index)
    .sort((left, right) => left.name.localeCompare(right.name));
  window.localStorage.setItem(customThemeStorageKey, JSON.stringify(normalized));
  return getAllWebThemes();
}

export function getAllWebThemes(): WebTheme[] {
  const defaultNames = new Set(webThemes.map((theme) => theme.name.toLowerCase()));
  legacyBuiltInThemeNames.forEach((name) => defaultNames.add(name.toLowerCase()));
  const customThemes = loadCustomWebThemes().filter((theme) => !defaultNames.has(theme.name.toLowerCase()));
  return [...webThemes, ...customThemes];
}

export function saveCustomWebTheme(theme: WebTheme) {
  const cleanTheme = normalizeWebTheme(theme);
  if (!cleanTheme.name) return getAllWebThemes();

  const defaultNames = new Set(webThemes.map((item) => item.name.toLowerCase()));
  legacyBuiltInThemeNames.forEach((name) => defaultNames.add(name.toLowerCase()));
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
  return normalizeWebTheme(getAllWebThemes().find((theme) => theme.name === normalized) ?? webThemes[0]);
}

function normalizeThemeName(name: string | null | undefined) {
  if (!name) return defaultThemeName;
  if (name === "Light") return "Mercy";
  if (name === "Dark" || name === "Modern") return "Gotham";
  return name;
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
