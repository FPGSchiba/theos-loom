/**
 * Theos UI runtime (window.TheosUI). The components are CSS classes in bundle.css; this API only
 * manages the theme, which is the `data-theme` attribute on the root element (or any element).
 */

/** A theme of the system. */
export interface TheosTheme {
  /** The value of `data-theme`. */
  id: "grid-noir" | "grid-light";
  /** Display name. */
  name: string;
}

export interface TheosUI {
  /** Both themes, the default first. */
  themes: TheosTheme[];
  /** "grid-noir". */
  defaultTheme: "grid-noir";
  /** The theme set on `el` (default: the root element), or the default theme. */
  getTheme(el?: Element): "grid-noir" | "grid-light";
  /** Sets the theme on `el` (default: the root element); unknown ids fall back to the default. Returns the id set. */
  setTheme(id: string, el?: Element): "grid-noir" | "grid-light";
  /** Switches to the other theme and returns the new id. */
  toggleTheme(el?: Element): "grid-noir" | "grid-light";
  /** grid-light when the OS prefers a light scheme, else grid-noir. The app decides whether to use it. */
  preferredTheme(): "grid-noir" | "grid-light";
}

declare global {
  interface Window {
    TheosUI: TheosUI;
  }
}
