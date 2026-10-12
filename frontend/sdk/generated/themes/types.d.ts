/** Everything the levels say for one ticket, the reader aside — what
[`resolve`] decides from. */
export interface Levels {
    /** What it says of its frame alone. */
    frame: Said;
    /** The nearest shortcode that says anything — the ticket's own, else
  the nearest hosting it (THEME-4) — and what it says. */
    shortcode?: ThemeSetting | null;
    /** The site's theme (`None`: default). */
    site?: string | null;
    /** What the ticket's frontmatter says of its view. */
    ticket: Said;
}
/** Who wears what: the site, the ticket view, the frame. */
export interface Regions {
    frame: Worn;
    site: Worn;
    ticket: Worn;
}
/** What one level says. */
export interface Said {
    reach: ThemeReach;
    takeover: Takeover;
    /** The theme it adopts, if any. */
    theme?: string | null;
}
/** Whether a level takes the themes from above (`on`, the default) or
ignores them (`off`). */
export type Takeover = "on" | "off";
/** A theme as the server keeps it: its name and its overrides, by token
name (`--accent`), for light and for dark. */
export interface Theme {
    dark: Record<string, unknown>;
    light: Record<string, unknown>;
    name: string;
    /** When it was last defined, unix seconds (0 for `default`). */
    updatedUnix: number;
}
/** The level a region's theme came from. */
export type ThemeLevel = never;
/** How far an adoption reaches. */
export type ThemeReach = "frame" | "ticket" | "site";
/** What the site or a shortcode says, as `themes.set` keeps it. */
export interface ThemeSetting {
    reach: ThemeReach;
    /** `site`, or a shortcode (upper case). */
    scope: string;
    takeover: Takeover;
    theme?: string | null;
}
/** A theme, as it is now (`define`, `get`, `list`; `watch` when one is
defined). */
export interface ThemesEventTheme {
    type: 'theme';
    theme: Theme;
}
/** What the site or a shortcode says, as it is now (`set`, `list`;
`watch` when it moves). Saying nothing — no theme, takeover on — is
the setting cleared. */
export interface ThemesEventSetting {
    type: 'setting';
    setting: ThemeSetting;
}
/** `theme_for`'s answer: each region's theme and the level it came
from, and what every level said. */
export interface ThemesEventRegions {
    type: 'regions';
    id: string;
    levels: Levels;
    regions: Regions;
}
/** A slow `watch` lost `missed` changes: `list` again. */
export interface ThemesEventLagged {
    type: 'lagged';
    missed: number;
}
/** In-band refusal, naming the problem; nothing was written. */
export interface ThemesEventError {
    type: 'error';
    message: string;
}
/** What the `themes` activation streams. Every method returns
`impl Stream<Item = ThemesEvent>`; a refusal is the in-band `Error`. */
export type ThemesEvent = ThemesEventTheme | ThemesEventSetting | ThemesEventRegions | ThemesEventLagged | ThemesEventError;
export declare function isThemesEventTheme(e: ThemesEvent): e is ThemesEventTheme;
export declare function isThemesEventSetting(e: ThemesEvent): e is ThemesEventSetting;
export declare function isThemesEventRegions(e: ThemesEvent): e is ThemesEventRegions;
export declare function isThemesEventLagged(e: ThemesEvent): e is ThemesEventLagged;
export declare function isThemesEventError(e: ThemesEvent): e is ThemesEventError;
/** What a region wears, and the level it came from. */
export interface Worn {
    /** The shortcode, for a shortcode's theme. */
    from?: string | null;
    level: ThemeLevel;
    theme: string;
}
