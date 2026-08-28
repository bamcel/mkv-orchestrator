# UI Color Themes

Reusable color reference for keeping MKV Orchestrator and related apps visually consistent.

Source of truth: [`web/src/theme.ts`](../web/src/theme.ts). The default theme is **Gotham**. All values are hexadecimal sRGB colors.

## Semantic color contract

| Token | CSS custom property | Intended use |
| --- | --- | --- |
| `Window` | `--color-window` | App and page background |
| `Card` | `--color-card` | Cards and raised surfaces |
| `Panel` | `--color-panel` | Panels and grouped regions |
| `Sidebar` | `--color-sidebar` | Navigation sidebar |
| `Input` | `--color-input` | Form controls |
| `InputHover` | `--color-input-hover` | Hovered form controls |
| `Button` | `--color-button` | Button background |
| `ButtonHover` | `--color-button-hover` | Hovered button background |
| `Selected` | `--color-selected` | Selected rows and navigation items |
| `Border` | `--color-border` | Standard borders and dividers |
| `BorderStrong` | `--color-border-strong` | Emphasized borders |
| `Text` | `--color-text` | Primary text |
| `Muted` | `--color-muted` | Secondary text |
| `Subtle` | `--color-subtle` | Tertiary text and quiet labels |
| `Accent` | `--color-accent` | Primary interactive accent |
| `AccentHover` | `--color-accent-hover` | Hovered accent |
| `TemplateHighlight` | `--color-template` | Template-specific highlight |
| `AppTitle` | `--color-app-title` | App title and logo |
| `Success` | `--color-success` | Success state |
| `Warning` | `--color-warning` | Warning state |
| `Disabled` | `--color-disabled` | Disabled state |
| `Brand` | `--color-brand` | Brand accent |

## Surface colors

| Theme | Window | Card | Panel | Sidebar | Input | InputHover | Button | ButtonHover | Selected |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Absolutely | `#F6F3EE` | `#FFFCF8` | `#F1ECE5` | `#E9E3DB` | `#F4EFE9` | `#DED2C7` | `#E8DED4` | `#DED2C7` | `#F0DDD0` |
| Cappuccin | `#1E1E2E` | `#252538` | `#2B2B40` | `#181825` | `#313147` | `#484864` | `#3B3B54` | `#484864` | `#403854` |
| Codex | `#181A1F` | `#202329` | `#252930` | `#15171B` | `#2A2E36` | `#3A424E` | `#303640` | `#3A424E` | `#263C4A` |
| Everforest | `#272E33` | `#2E383C` | `#343F44` | `#232A2E` | `#374145` | `#4B565C` | `#414B50` | `#4B565C` | `#3C4F48` |
| GitHub | `#0D1117` | `#161B22` | `#1C2128` | `#010409` | `#0D1117` | `#30363D` | `#21262D` | `#30363D` | `#1F3042` |
| Gotham | `#15171C` | `#20232A` | `#252932` | `#1B1E25` | `#1D2028` | `#292E38` | `#3B4252` | `#2E3440` | `#2E3440` |
| Gruvbox | `#282828` | `#32302F` | `#3C3836` | `#1D2021` | `#3C3836` | `#665C54` | `#504945` | `#665C54` | `#4A4435` |
| Linear | `#17171A` | `#1F1F23` | `#25252A` | `#121214` | `#29292F` | `#3A3A44` | `#303038` | `#3A3A44` | `#343047` |
| Mercy | `#F5F6FA` | `#E8ECF4` | `#EEF1F7` | `#E8ECF4` | `#E8ECF4` | `#F1F4FA` | `#DCE3EF` | `#6D5BD0` | `#D9DDF0` |
| Midnight | `#1E1F29` | `#282A36` | `#2B2E3A` | `#232631` | `#282A36` | `#2F3140` | `#44475A` | `#3A3D4F` | `#3A3D4F` |
| Notion | `#F7F7F5` | `#FFFFFF` | `#F1F1EF` | `#F0F0EE` | `#F7F7F5` | `#DEDEDB` | `#E9E9E7` | `#DEDEDB` | `#E3ECF4` |
| One | `#21252B` | `#282C34` | `#2C313A` | `#1E2228` | `#2F343E` | `#464D59` | `#3A404B` | `#464D59` | `#333F55` |
| Proof | `#F4F5F0` | `#FCFDF9` | `#ECEFE8` | `#E8EBE5` | `#F2F4EF` | `#D5DDD7` | `#E1E6DF` | `#D5DDD7` | `#DDEAE3` |
| Raycast | `#171719` | `#202024` | `#27272C` | `#121214` | `#2B2B31` | `#414149` | `#35353C` | `#414149` | `#493137` |
| Rose Pine | `#191724` | `#1F1D2E` | `#26233A` | `#14121E` | `#26233A` | `#3B3752` | `#312E45` | `#3B3752` | `#403044` |
| Solarized | `#FDF6E3` | `#EEE8D5` | `#F7F0DC` | `#E8E2CF` | `#F5EEDB` | `#D2CCB9` | `#DED8C5` | `#D2CCB9` | `#DDE8E4` |
| Vercel | `#0A0A0A` | `#111111` | `#171717` | `#050505` | `#1A1A1A` | `#303030` | `#242424` | `#303030` | `#16263B` |
| VS Code Plus | `#181818` | `#1F1F1F` | `#252526` | `#181818` | `#2A2D2E` | `#3E3E42` | `#333337` | `#3E3E42` | `#24394A` |
| Xcode | `#F2F4F7` | `#FFFFFF` | `#E9EDF2` | `#E5E9EF` | `#F7F8FA` | `#D2D8E0` | `#DFE4EA` | `#D2D8E0` | `#DCEBFA` |

## Content and border colors

| Theme | Border | BorderStrong | Text | Muted | Subtle | Disabled |
| --- | --- | --- | --- | --- | --- | --- |
| Absolutely | `#D6CCC2` | `#B9AA9D` | `#292522` | `#655D56` | `#887D74` | `#A79D95` |
| Cappuccin | `#45455F` | `#62627C` | `#CDD6F4` | `#BAC2DE` | `#9399B2` | `#6C7086` |
| Codex | `#3A414C` | `#566170` | `#F1F3F5` | `#C5CAD1` | `#929AA5` | `#69727E` |
| Everforest | `#475258` | `#68757A` | `#D3C6AA` | `#B7B09A` | `#859289` | `#6D7A72` |
| GitHub | `#30363D` | `#484F58` | `#E6EDF3` | `#B1BAC4` | `#7D8590` | `#6E7681` |
| Gotham | `#3B4252` | `#4C566A` | `#ECEFF4` | `#D8DEE9` | `#A7B0C0` | `#7D8797` |
| Gruvbox | `#504945` | `#7C6F64` | `#EBDBB2` | `#D5C4A1` | `#A89984` | `#7C6F64` |
| Linear | `#35353D` | `#51515E` | `#F1F1F3` | `#C5C5CC` | `#8B8B96` | `#676771` |
| Mercy | `#CAD2E0` | `#9DA8BA` | `#1C2430` | `#46556A` | `#66758A` | `#8792A3` |
| Midnight | `#343746` | `#44475A` | `#F8F8F2` | `#CFCFEA` | `#8B93A7` | `#6272A4` |
| Notion | `#D9D9D6` | `#B6B6B2` | `#252525` | `#5F5E5A` | `#85847F` | `#A09F9A` |
| One | `#3E4451` | `#5C6370` | `#ABB2BF` | `#C8CDD5` | `#7F8794` | `#5C6370` |
| Proof | `#CED6D0` | `#AAB8AF` | `#26332D` | `#52645A` | `#77877E` | `#98A39D` |
| Raycast | `#3D3D44` | `#5B5B64` | `#FAFAFA` | `#D0CFD3` | `#929198` | `#6D6C73` |
| Rose Pine | `#393552` | `#6E6A86` | `#E0DEF4` | `#C4A7E7` | `#908CAA` | `#6E6A86` |
| Solarized | `#D0C9B5` | `#93A1A1` | `#073642` | `#586E75` | `#839496` | `#A4AAA4` |
| Vercel | `#2E2E2E` | `#505050` | `#EDEDED` | `#B7B7B7` | `#888888` | `#666666` |
| VS Code Plus | `#3C3C3C` | `#5A5A5A` | `#CCCCCC` | `#B8B8B8` | `#858585` | `#666666` |
| Xcode | `#CBD1D9` | `#A3ABB6` | `#1F2328` | `#505963` | `#747E89` | `#99A1AA` |

## Accent and state colors

| Theme | Accent | AccentHover | TemplateHighlight | AppTitle | Brand | Success | Warning |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Absolutely | `#C66B43` | `#AD5835` | `#C66B43` | `#C66B43` | `#C66B43` | `#347A4A` | `#9A6218` |
| Cappuccin | `#CBA6F7` | `#B58BE8` | `#CBA6F7` | `#CBA6F7` | `#CBA6F7` | `#A6E3A1` | `#F9E2AF` |
| Codex | `#3B9EFF` | `#2188E8` | `#3B9EFF` | `#3B9EFF` | `#3B9EFF` | `#42C77A` | `#E7B65A` |
| Everforest | `#A7C080` | `#91AD6C` | `#A7C080` | `#A7C080` | `#A7C080` | `#83C092` | `#DBBC7F` |
| GitHub | `#2F81F7` | `#1F6FEB` | `#2F81F7` | `#2F81F7` | `#2F81F7` | `#3FB950` | `#D29922` |
| Gotham | `#BD93F9` | `#2E3440` | `#BD93F9` | `#BD93F9` | `#BD93F9` | `#50FA7B` | `#EBCB8B` |
| Gruvbox | `#D79921` | `#B57614` | `#D79921` | `#D79921` | `#D79921` | `#98971A` | `#FE8019` |
| Linear | `#7C6AEF` | `#6957DC` | `#7C6AEF` | `#7C6AEF` | `#7C6AEF` | `#4AC58B` | `#E0AA55` |
| Mercy | `#6D5BD0` | `#6D5BD0` | `#6D5BD0` | `#6D5BD0` | `#6D5BD0` | `#17803D` | `#A15C00` |
| Midnight | `#BD93F9` | `#3A3D4F` | `#BD93F9` | `#BD93F9` | `#BD93F9` | `#50FA7B` | `#FFA500` |
| Notion | `#0B6E99` | `#095E83` | `#0B6E99` | `#0B6E99` | `#0B6E99` | `#448361` | `#A66A18` |
| One | `#61AFEF` | `#4D9BD8` | `#61AFEF` | `#61AFEF` | `#61AFEF` | `#98C379` | `#E5C07B` |
| Proof | `#3D7660` | `#315F4D` | `#3D7660` | `#3D7660` | `#3D7660` | `#3C805A` | `#95691F` |
| Raycast | `#FF5A67` | `#E94B58` | `#FF5A67` | `#FF5A67` | `#FF5A67` | `#55C994` | `#E4B45E` |
| Rose Pine | `#EB6F92` | `#D85F82` | `#EB6F92` | `#EB6F92` | `#EB6F92` | `#9CCFD8` | `#F6C177` |
| Solarized | `#B58900` | `#987400` | `#B58900` | `#B58900` | `#B58900` | `#2AA198` | `#CB4B16` |
| Vercel | `#0070F3` | `#0060D1` | `#0070F3` | `#0070F3` | `#0070F3` | `#46A758` | `#E5A000` |
| VS Code Plus | `#007ACC` | `#006BB3` | `#007ACC` | `#007ACC` | `#007ACC` | `#4EC9B0` | `#DCDCAA` |
| Xcode | `#006FE6` | `#005FC7` | `#006FE6` | `#006FE6` | `#006FE6` | `#348A42` | `#A86400` |

## Reuse in another app

Keep the semantic token names even if the target framework uses a different format. This lets components ask for a purpose such as `Selected` or `Muted` instead of depending on a specific hex value.

For CSS-based apps, copy the CSS custom-property names from the contract table and assign the values from one theme. For JSON-based apps, use this shape:

```json
{
  "name": "Theme Name",
  "colors": {
    "Window": "#000000",
    "Card": "#111111",
    "Accent": "#0070F3",
    "Text": "#FFFFFF"
  }
}
```

When adding a new theme, define all 22 roles. `TemplateHighlight`, `AppTitle`, and `Brand` commonly match `Accent`, but remain separate tokens so an app can customize them later.
