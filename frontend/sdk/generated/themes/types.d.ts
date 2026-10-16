import type { WatchEnd } from '../chats/types';
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
/** Whether a level takes the themes from above (any case; true/yes, false/no too) */
export type Takeover = "on" | "off";
/** A theme as the server keeps it: its name and its overrides, by token
name (`--accent`), for light and for dark — what it says itself; the
rest comes from the theme it `extends` ([`flatten`]). THEME-3: a theme
is a ticket's ` ```theme ` block ([`crate::theme_block`]), owned as a
component is (COMP-3's rules). */
export interface Theme {
    dark: Record<string, unknown>;
    /** THEME-3: the theme it states its differences from (`None`: `default`). */
    extends?: string | null;
    /** THEME-3: the https stylesheets it imports (a font's). */
    imports?: string[];
    light: Record<string, unknown>;
    name: string;
    /** THEME-3: its block is gone from its owner's body; it is kept,
  read-only, while something uses it (a setting, a ticket's
  frontmatter, a live theme extending it), and a block naming it adopts it. */
    orphan?: boolean;
    /** THEME-3: the ticket whose body's block defines it — `None` for one
  `themes.define` made, or an orphan. */
    owner?: string | null;
    /** THEME-3: its `@font-face` and `m-*::part(…)` rules, as validated. */
    rules?: string[];
    /** When it was last defined, unix seconds (0 for `default`). */
    updatedUnix: number;
}
/** The level a region's theme came from. */
export type ThemeLevel = never;
/** THEME-8: a theme's name — lower case letters, digits and `-`,
starting with a letter, at most 40 (`ocean`): [`crate::themes::name_of`]. */
export type ThemeName = string;
/** How far a theme's adoption reaches (any case) */
export type ThemeReach = "frame" | "ticket" | "site";
/** THEME-4: where a theme is adopted — `site`, or a shortcode by any
name [`ShortcodeRef`] takes. */
export type ThemeScope = string;
/** What the site or a shortcode says, as `themes.set` keeps it. */
export interface ThemeSetting {
    reach: ThemeReach;
    /** `site`, or a shortcode (upper case). */
    scope: string;
    takeover: Takeover;
    theme?: string | null;
}
/** A theme, as it is now (`define`, `get`, `list`). */
export interface ThemesEventTheme {
    type: 'theme';
    theme: Theme;
}
/** THEME-3 (`watch`): a theme defined, its block written or amended,
orphaned or adopted — as it is now. */
export interface ThemesEventThemeChanged {
    type: 'theme_changed';
    theme: Theme;
}
/** THEME-3 (`watch`): a theme left the registry — an orphan nothing
uses any more. */
export interface ThemesEventThemeRemoved {
    type: 'theme_removed';
    name: string;
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
/** ACCESS-12 (`watch`): the watch ended — its caller, verified again
when the grants moved or its sign-in ended, was refused. Last. */
export interface ThemesEventWatchClosed {
    type: 'watch_closed';
    message: string;
    reason: WatchEnd;
}
/** In-band refusal, naming the problem; nothing was written. */
export interface ThemesEventError {
    type: 'error';
    message: string;
}
/** What the `themes` activation streams. Every method returns
`impl Stream<Item = ThemesEvent>`; a refusal is the in-band `Error`. */
export type ThemesEvent = ThemesEventTheme | ThemesEventThemeChanged | ThemesEventThemeRemoved | ThemesEventSetting | ThemesEventRegions | ThemesEventLagged | ThemesEventWatchClosed | ThemesEventError;
export declare function isThemesEventTheme(e: ThemesEvent): e is ThemesEventTheme;
export declare function isThemesEventThemeChanged(e: ThemesEvent): e is ThemesEventThemeChanged;
export declare function isThemesEventThemeRemoved(e: ThemesEvent): e is ThemesEventThemeRemoved;
export declare function isThemesEventSetting(e: ThemesEvent): e is ThemesEventSetting;
export declare function isThemesEventRegions(e: ThemesEvent): e is ThemesEventRegions;
export declare function isThemesEventLagged(e: ThemesEvent): e is ThemesEventLagged;
export declare function isThemesEventWatchClosed(e: ThemesEvent): e is ThemesEventWatchClosed;
export declare function isThemesEventError(e: ThemesEvent): e is ThemesEventError;
/** THEME-8: what a theme sets a token to (`#00796b`) — not blank, at
most 200 bytes, nothing that could end its declaration:
[`crate::themes::value_ok`]. */
export type TokenValue = string;
/** What a region wears, and the level it came from. */
export interface Worn {
    /** The shortcode, for a shortcode's theme. */
    from?: string | null;
    level: ThemeLevel;
    theme: string;
}
