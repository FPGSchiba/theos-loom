/* @ds-bundle: {"format":4,"namespace":"TheosUI","components":[]} */
/* Theos UI — runtime helpers. The components themselves are plain CSS classes (bundle.css);
   this script only manages the theme. No framework, no network. */
(function () {
  "use strict";
  var THEMES = [
    { id: "grid-noir", name: "Grid Noir" },
    { id: "grid-light", name: "Grid Light" }
  ];
  var DEFAULT = THEMES[0].id;

  function isTheme(id) {
    for (var i = 0; i < THEMES.length; i++) if (THEMES[i].id === id) return true;
    return false;
  }

  /** The theme set on `el` (default: the root element), or the default theme. */
  function getTheme(el) {
    var node = el || document.documentElement;
    var id = node.getAttribute("data-theme");
    return isTheme(id) ? id : DEFAULT;
  }

  /** Sets a theme on `el` (default: the root element). Unknown ids fall back to the default. */
  function setTheme(id, el) {
    var node = el || document.documentElement;
    var next = isTheme(id) ? id : DEFAULT;
    node.setAttribute("data-theme", next);
    return next;
  }

  /** Switches `el` (default: the root element) to the other theme and returns the new id. */
  function toggleTheme(el) {
    return setTheme(getTheme(el) === "grid-noir" ? "grid-light" : "grid-noir", el);
  }

  /** The theme that matches the system preference: grid-light for a light OS, else grid-noir. */
  function preferredTheme() {
    try {
      return window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches ? "grid-light" : DEFAULT;
    } catch (e) {
      return DEFAULT;
    }
  }

  window.TheosUI = {
    themes: THEMES.slice(),
    defaultTheme: DEFAULT,
    getTheme: getTheme,
    setTheme: setTheme,
    toggleTheme: toggleTheme,
    preferredTheme: preferredTheme
  };
})();
